// Pure helpers for the Worker (src/worker.js), kept apart so it exports only its handler:
// workerd treats every named export of the main module as an entrypoint.
import release from "../release.json" with { type: "json" };

/** The site moved here from voxsplit.triplejmusic.art, which now redirects. */
export const HOME = "voxsplit.jeffrey.build";

/** Old-host URLs go to the same path on the new host (curl -L follows it, so copied
    install commands keep working). */
export function movedTo(url) {
  return url.hostname === "voxsplit.triplejmusic.art" ? `https://${HOME}${url.pathname}${url.search}` : null;
}

export const pkgPath = (r = release) => `/download/${r.file}`;

export const megabytes = (bytes) => `${Math.round(bytes / 1_000_000)} MB`;

/** R2 reports `{offset, length}` or `{suffix}`; normalise to offset/length. */
export function rangeOf(range, size) {
  if ("suffix" in range) {
    const length = Math.min(range.suffix, size);
    return { offset: size - length, length };
  }
  const offset = range.offset ?? 0;
  return { offset, length: range.length ?? size - offset };
}

/** Downloads are always https, except when developing locally. */
export function publicOrigin(url) {
  return url.hostname === "localhost" || url.hostname === "127.0.0.1" ? url.origin : `https://${url.host}`;
}

export function fillScript(template, origin, r = release) {
  return template
    .replaceAll("__VERSION__", r.version)
    .replaceAll("__URL__", `${origin}${pkgPath(r)}`)
    .replaceAll("__SHA256__", r.sha256)
    .replaceAll("__SIZE__", megabytes(r.bytes));
}
