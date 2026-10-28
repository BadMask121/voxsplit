"""Export a BS-RoFormer checkpoint to ONNX with the STFT moved to the host.

The exported graph takes the stereo STFT of one chunk as a real tensor and
returns the complex mask for the model's target stem. voxsplit-engine computes
the STFT, multiplies by the mask, zeroes the DC bin and runs the iSTFT, exactly
as BSRoformer.forward does (vendor/bs_roformer.py).

    stft  float32 [1, 2*F, T, 2]   index f*2 + channel; last axis = (re, im)
    mask  float32 [1, 2*F, T, 2]

Developer-only; end users never need Python. Usage (from the repo root):

    UV_PROJECT_ENVIRONMENT=~/Library/Caches/VoxSplit/roformer-venv \\
    uv run --project tools/roformer python -I tools/roformer/export.py \\
        --config <config.yaml> --checkpoint <model.ckpt> --out models/<Name>.onnx
"""

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import numpy as np  # noqa: E402
import torch  # noqa: E402
import yaml  # noqa: E402
from einops import rearrange  # noqa: E402

from vendor.bs_roformer import BSRoformer  # noqa: E402


def load_config(path: Path) -> dict:
    """SafeLoader plus python/tuple (the configs use it); never yaml.unsafe_load on a download."""

    class Loader(yaml.SafeLoader):
        pass

    Loader.add_constructor("tag:yaml.org,2002:python/tuple", lambda loader, node: tuple(loader.construct_sequence(node)))
    return yaml.load(path.read_text(), Loader=Loader)


def build_model(config: dict, checkpoint: Path) -> BSRoformer:
    kwargs = dict(config["model"])
    kwargs["flash_attn"] = False  # plain softmax attention exports cleanly
    model = BSRoformer(**kwargs)
    # weights_only: the checkpoint is a downloaded pickle, so never let it run code.
    state = torch.load(checkpoint, map_location="cpu", weights_only=True)
    if isinstance(state, dict) and "state_dict" in state:
        state = state["state_dict"]
    missing, unexpected = model.load_state_dict(state, strict=False)
    if missing or unexpected:
        raise SystemExit(f"checkpoint/code mismatch\n  missing: {missing[:8]}\n  unexpected: {unexpected[:8]}")
    return model.eval()


def make_coreml_friendly() -> None:
    """Swap two constructs ONNX Runtime's CoreML provider rejects for exact equivalents.

    - rotary_embedding_torch slices out empty [0:0] / [64:64] ranges and concatenates
      single tensors; CoreML refuses both, splitting the graph into ~170 partitions.
      Rotation becomes t*cos + (t @ R)*sin with precomputed tables and a constant
      pair-swap matrix R (rotate_half as a matmul).
    - einsum attention becomes two matmuls.
    - RMSNorm's F.normalize (ReduceL2 + Expand, unsupported) becomes
      x / sqrt(max(sum(x^2), eps^2)), which is the same function.
    The parity checks in main() prove the exported graph still matches the model.
    """
    import rotary_embedding_torch

    from vendor import attend, bs_roformer

    tables: dict = {}

    def rotate_queries_or_keys(self, t, seq_dim=None, offset=0, scale=None):
        assert offset == 0 and scale is None and not self.use_xpos
        n, d = t.shape[-2], t.shape[-1]
        key = (id(self), n, d)
        if key not in tables:
            with torch.no_grad():
                freqs = self.forward(torch.arange(n, dtype=torch.float32), seq_len=n)
                assert freqs.shape[-1] == d, "partial rotary dims are not supported"
                swap = torch.zeros(d, d)
                for i in range(0, d, 2):  # (x0, x1) -> (-x1, x0), as rotate_half does
                    swap[i + 1, i] = -1.0
                    swap[i, i + 1] = 1.0
                tables[key] = (freqs.cos().detach(), freqs.sin().detach(), swap)
        cos, sin, swap = tables[key]
        return t * cos + torch.matmul(t, swap) * sin

    def attend_forward(self, q, k, v):
        scale = self.scale if self.scale is not None else q.shape[-1] ** -0.5
        attn = (torch.matmul(q, k.transpose(-1, -2)) * scale).softmax(dim=-1)
        return torch.matmul(attn, v)

    def rms_norm_forward(self, x):
        norm = torch.sqrt(torch.clamp((x * x).sum(dim=-1, keepdim=True), min=1e-24))  # F.normalize eps=1e-12
        return x / norm * self.scale * self.gamma

    rotary_embedding_torch.RotaryEmbedding.rotate_queries_or_keys = rotate_queries_or_keys
    attend.Attend.forward = attend_forward
    bs_roformer.RMSNorm.forward = rms_norm_forward


class RoformerCore(torch.nn.Module):
    """BSRoformer.forward between the STFT and the mask multiply, for the first stem."""

    def __init__(self, model: BSRoformer):
        super().__init__()
        assert not model.skip_connection, "skip_connection models are not supported"
        self.model = model

    def forward(self, stft: torch.Tensor) -> torch.Tensor:
        # Plain reshapes instead of einops.pack: pack emits single-input Concats
        # that CoreML rejects.
        m = self.model
        x = rearrange(stft, "b f t c -> b t (f c)")
        x = m.band_split(x)  # [b, t, bands, d]
        b, t, bands, d = x.shape
        for block in m.layers:
            if len(block) == 3:
                linear_transformer, time_transformer, freq_transformer = block
                x = linear_transformer(x.reshape(b, t * bands, d)).reshape(b, t, bands, d)
            else:
                time_transformer, freq_transformer = block
            x = x.permute(0, 2, 1, 3).reshape(b * bands, t, d)
            x = time_transformer(x)
            x = x.reshape(b, bands, t, d).permute(0, 2, 1, 3)
            x = freq_transformer(x.reshape(b * t, bands, d)).reshape(b, t, bands, d)
        x = m.final_norm(x)
        mask = m.mask_estimators[0](x)
        return rearrange(mask, "b t (f c) -> b f t c", c=2)


def host_stft(model: BSRoformer, audio: torch.Tensor) -> torch.Tensor:
    """What the engine computes: [2, samples] -> [1, 2*F, T, 2]."""
    window = model.stft_window_fn(device="cpu")
    spec = torch.stft(audio, **model.stft_kwargs, window=window, return_complex=True)
    spec = torch.view_as_real(spec)  # [s, f, t, c]
    return rearrange(spec, "s f t c -> 1 (f s) t c")


def host_apply(model: BSRoformer, stft: torch.Tensor, mask: torch.Tensor, length: int) -> torch.Tensor:
    """What the engine does with the mask: complex multiply, zero DC, iSTFT -> [2, samples]."""
    spec = torch.view_as_complex(stft.contiguous()) * torch.view_as_complex(mask.contiguous())
    spec = rearrange(spec, "1 (f s) t -> s f t", s=2)
    spec[:, 0] = 0.0
    window = model.stft_window_fn(device="cpu")
    return torch.istft(spec, **model.stft_kwargs, window=window, length=length)


def snr(reference: np.ndarray, estimate: np.ndarray) -> float:
    return float(10 * np.log10(np.sum(reference**2) / max(np.sum((reference - estimate) ** 2), 1e-30)))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--checkpoint", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--chunk-seconds", type=float, default=8.0,
                        help="chunk length baked into the graph (attention cost grows with its square)")
    args = parser.parse_args()

    config = load_config(args.config)
    model = build_model(config, args.checkpoint)
    hop = model.stft_kwargs["hop_length"]
    rate = config["audio"]["sample_rate"]
    # A whole number of hops, so the iSTFT returns exactly one chunk.
    chunk = int(round(args.chunk_seconds * rate / hop)) * hop
    core = RoformerCore(model).eval()

    torch.manual_seed(0)
    t = torch.arange(chunk) / rate
    audio = torch.stack([0.3 * torch.sin(2 * torch.pi * 220 * t) + 0.05 * torch.randn(chunk),
                         0.3 * torch.sin(2 * torch.pi * 330 * t) + 0.05 * torch.randn(chunk)])

    with torch.no_grad():
        # Reference from the unmodified model, before the CoreML-friendly swaps.
        reference = model(audio.unsqueeze(0))[0].numpy()  # full PyTorch forward, [2, samples]
        make_coreml_friendly()
        stft = host_stft(model, audio)
        mask = core(stft)
        rebuilt = host_apply(model, stft, mask, chunk).numpy()
    print(f"chunk {chunk} samples ({chunk / rate:.2f} s), stft {tuple(stft.shape)}")
    print(f"wrapper vs full forward: {snr(reference, rebuilt):.1f} dB SNR")

    args.out.parent.mkdir(parents=True, exist_ok=True)
    torch.onnx.export(core, (stft,), str(args.out), input_names=["stft"], output_names=["mask"],
                      opset_version=17, dynamo=False, do_constant_folding=True)

    import onnxruntime as ort

    session = ort.InferenceSession(str(args.out), providers=["CPUExecutionProvider"])
    onnx_mask = session.run(None, {"stft": stft.numpy()})[0]
    with torch.no_grad():
        onnx_audio = host_apply(model, stft, torch.from_numpy(onnx_mask), chunk).numpy()
    print(f"onnx mask max |diff|: {np.max(np.abs(onnx_mask - mask.numpy())):.2e}")
    print(f"onnx path vs full forward: {snr(reference, onnx_audio):.1f} dB SNR")
    print(f"wrote {args.out} ({args.out.stat().st_size / 1e6:.0f} MB); chunk_len={chunk} frames={stft.shape[2]}")


if __name__ == "__main__":
    main()
