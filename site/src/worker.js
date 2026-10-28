// voxsplit.jeffrey.build: fills the current release into the page and the install
// script, serves installers from R2 (with resumable range requests), and redirects
// the site's first home, voxsplit.triplejmusic.art.
import release from "../release.json" with { type: "json" };

export const pkgPath = (r = release) => `/download/${r.file}`;
export const megabytes = (bytes) => `${Math.round(bytes / 1_000_000)} MB`;

/** The site moved here from voxsplit.triplejmusic.art, which now redirects. */
export const HOME = "voxsplit.jeffrey.build";

/** Old-host URLs go to the same path on the new host (curl -L follows it, so copied
    install commands keep working). */
export function movedTo(url) {
  return url.hostname === "voxsplit.triplejmusic.art" ? `https://${HOME}${url.pathname}${url.search}` : null;
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const moved = movedTo(url);
    if (moved) {
      return Response.redirect(moved, 301);
    }
    const { pathname } = url;

    if (pathname === "/download/latest") {
      return Response.redirect(new URL(pkgPath(), url).toString(), 302);
    }
    if (pathname.startsWith("/download/")) {
      return download(request, env, decodeURIComponent(pathname.slice("/download/".length)));
    }
    if (pathname === "/install.sh") {
      return installScript(request, env, url);
    }
    if (pathname === "/") {
      return page(request, env, url);
    }
    return env.ASSETS.fetch(request);
  },
};

async function download(request, env, key) {
  if (request.method !== "GET" && request.method !== "HEAD") {
    return new Response("Method not allowed", { status: 405, headers: { Allow: "GET, HEAD" } });
  }
  if (key === `${release.file}.sha256` || key === "latest.sha256") {
    return new Response(`${release.sha256}  ${release.file}\n`, {
      headers: { "Content-Type": "text/plain; charset=utf-8", "Cache-Control": "public, max-age=300" },
    });
  }
  // Only published files, never arbitrary bucket keys.
  if (!/^VoxSplit-\d+\.\d+\.\d+\.pkg$/.test(key)) {
    return notFound(env, request);
  }

  const object =
    request.method === "HEAD"
      ? await env.DOWNLOADS.head(key)
      : await env.DOWNLOADS.get(key, { range: request.headers, onlyIf: request.headers });
  if (object === null) {
    return notFound(env, request);
  }

  const headers = new Headers();
  object.writeHttpMetadata(headers);
  headers.set("ETag", object.httpEtag);
  headers.set("Accept-Ranges", "bytes");
  headers.set("Content-Type", "application/octet-stream");
  headers.set("Content-Disposition", `attachment; filename="${key}"`);
  // Versioned file names never change content.
  headers.set("Cache-Control", "public, max-age=31536000, immutable");

  if (request.method === "HEAD") {
    headers.set("Content-Length", String(object.size));
    return new Response(null, { headers });
  }
  // `onlyIf` failed (e.g. If-None-Match matched): the object has no body.
  if (!("body" in object)) {
    return new Response(null, { status: 304, headers });
  }
  if (object.range) {
    const { offset = 0, length = object.size - offset } = rangeOf(object.range, object.size);
    headers.set("Content-Range", `bytes ${offset}-${offset + length - 1}/${object.size}`);
    headers.set("Content-Length", String(length));
    return new Response(object.body, { status: 206, headers });
  }
  headers.set("Content-Length", String(object.size));
  return new Response(object.body, { headers });
}

/** R2 reports `{offset, length}` or `{suffix}`; normalise to offset/length. */
export function rangeOf(range, size) {
  if ("suffix" in range) {
    const length = Math.min(range.suffix, size);
    return { offset: size - length, length };
  }
  const offset = range.offset ?? 0;
  return { offset, length: range.length ?? size - offset };
}

async function installScript(request, env, url) {
  const asset = await env.ASSETS.fetch(new Request(new URL("/install.template.sh", url), request));
  if (!asset.ok) {
    return asset;
  }
  const script = fillScript(await asset.text(), publicOrigin(url));
  return new Response(script, {
    headers: {
      "Content-Type": "text/x-shellscript; charset=utf-8",
      "Cache-Control": "public, max-age=300",
      "X-Content-Type-Options": "nosniff",
    },
  });
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

async function page(request, env, url) {
  // "/" (not "/index.html", which the asset handler redirects to "/").
  const asset = await env.ASSETS.fetch(new Request(new URL("/", url), request));
  if (!asset.ok || !asset.headers.get("Content-Type")?.includes("text/html")) {
    return asset;
  }
  const text = (selector, value) => [selector, { element: (el) => el.setInnerContent(value) }];
  const rewrites = [
    text("[data-release=version]", release.version),
    text("[data-release=size]", megabytes(release.bytes)),
    text("[data-release=sha256]", release.sha256),
    text("[data-release=file]", release.file),
    text("[data-release=date]", release.date),
    text("[data-release=pkg-url]", `${publicOrigin(url)}${pkgPath()}`),
    ["a[data-release-href]", { element: (el) => el.setAttribute("href", pkgPath()) }],
  ];
  const rewriter = rewrites.reduce((rw, [selector, handler]) => rw.on(selector, handler), new HTMLRewriter());
  const response = rewriter.transform(asset);
  const headers = new Headers(response.headers);
  headers.set("Cache-Control", "public, max-age=300");
  return new Response(response.body, { status: response.status, headers });
}

async function notFound(env, request) {
  const page = await env.ASSETS.fetch(new Request(new URL("/404.html", request.url), request));
  return new Response(page.body, { status: 404, headers: page.headers });
}
