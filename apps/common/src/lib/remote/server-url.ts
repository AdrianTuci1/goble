export type NormalizedServer =
  | { ok: true; url: string }
  | { ok: false; error: string };

export function normalizeServerUrl(input: string): NormalizedServer {
  const trimmed = input.trim();
  if (!trimmed) return { ok: false, error: "Enter a gateway URL" };
  const withScheme = /^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed)
    ? trimmed
    : `https://${trimmed}`;

  let parsed: URL;
  try {
    parsed = new URL(withScheme);
  } catch {
    return { ok: false, error: "That does not look like a URL" };
  }
  if (parsed.protocol !== "https:" && parsed.protocol !== "http:") {
    return { ok: false, error: "Use an http or https URL" };
  }
  if (parsed.protocol === "http:" && !isLocalHost(parsed.hostname)) {
    return { ok: false, error: "Public gateways need https://" };
  }
  if (!parsed.hostname) return { ok: false, error: "That URL is missing a host" };
  return { ok: true, url: `${parsed.protocol}//${parsed.host}` };
}

export function isLocalHost(hostname: string): boolean {
  const host = hostname.replace(/^\[|\]$/g, "").toLowerCase();
  return (
    host === "localhost" ||
    host === "127.0.0.1" ||
    host === "::1" ||
    host.endsWith(".local") ||
    /^10(?:\.\d{1,3}){3}$/.test(host) ||
    /^192\.168(?:\.\d{1,3}){2}$/.test(host) ||
    /^172\.(1[6-9]|2\d|3[0-1])(?:\.\d{1,3}){2}$/.test(host)
  );
}
