import assert from "node:assert/strict";
import test from "node:test";

import {
  LATEST,
  bearerToken,
  downloadTarget,
  matchRoute,
  tokenMatches,
  validateAsset,
  validateTag,
} from "../src/route.mjs";

const REPO = "goble-dev/goble";
const ASSET = "Goble-0.2.0-arm64.dmg";

test("the known routes match and nothing else does", () => {
  assert.deepEqual(matchRoute("/"), { kind: "index" });
  assert.deepEqual(matchRoute("/health"), { kind: "health" });
  assert.deepEqual(matchRoute("/channel_versions.json"), { kind: "manifest" });
  assert.deepEqual(matchRoute("/admin/publish"), { kind: "publish" });
  assert.deepEqual(matchRoute(`/dl/v0.2.0/${ASSET}`), {
    kind: "download",
    tag: "v0.2.0",
    asset: ASSET,
  });
  assert.deepEqual(matchRoute("/dl/latest/Goble-macos.dmg"), {
    kind: "download",
    tag: LATEST,
    asset: "Goble-macos.dmg",
  });
});

test("unknown paths are notFound", () => {
  for (const pathname of ["/nope", "/healthz", "/dl/v0.2.0", "/dl", "/admin", "/admin/publish/extra", "/dl/a/b/c"]) {
    assert.equal(matchRoute(pathname).kind, "notFound", pathname);
  }
});

test("broken percent-encoding is a bad request, not a route", () => {
  const route = matchRoute("/dl/v0.2.0/%ZZ");
  assert.equal(route.kind, "badRequest");
});

test("trailing and leading slashes do not invent segments", () => {
  assert.equal(matchRoute("/health/").kind, "health");
  assert.equal(matchRoute("//health").kind, "health");
  assert.equal(matchRoute("/dl/v0.2.0/").kind, "notFound");
});

test("valid tags are accepted", () => {
  for (const tag of ["v0.2.0", "v1.0.0-beta.1", "v2", LATEST, "v0.2.0_rc1"]) {
    assert.equal(validateTag(tag), true, tag);
  }
});

test("invalid tags are refused", () => {
  for (const tag of ["", "v", "0.2.0", "V0.2.0", "LATEST", "latest/", "v0.2.0/x", "v..", "v0.2.0 beta", "v0.2.0?x", null, 2]) {
    assert.equal(validateTag(tag), false, String(tag));
  }
});

test("valid assets are accepted", () => {
  for (const asset of [ASSET, "Goble-0.2.0-amd64.AppImage", "latest.json", "a"]) {
    assert.equal(validateAsset(asset), true, asset);
  }
});

test("invalid assets are refused", () => {
  for (const asset of ["", "..", ".", "../etc/passwd", "a/b", "a\\b", "a b", "a?b", "a#b", "a%2Fb", null]) {
    assert.equal(validateAsset(asset), false, String(asset));
  }
});

test("a version tag redirects into the tagged release", () => {
  assert.equal(
    downloadTarget(REPO, "v0.2.0", ASSET),
    "https://github.com/goble-dev/goble/releases/download/v0.2.0/Goble-0.2.0-arm64.dmg",
  );
});

test("the latest tag redirects through GitHub's latest release", () => {
  assert.equal(
    downloadTarget(REPO, LATEST, ASSET),
    "https://github.com/goble-dev/goble/releases/latest/download/Goble-0.2.0-arm64.dmg",
  );
});

test("traversal, separators and empty segments have no download target", () => {
  const attempts = [
    [REPO, "v0.2.0", ".."],
    [REPO, "v0.2.0", "."],
    [REPO, "v0.2.0", "../etc/passwd"],
    [REPO, "v0.2.0", "..%2F..%2Fetc"],
    [REPO, "..", "Goble.dmg"],
    [REPO, "v0.2.0/../../x", "Goble.dmg"],
    [REPO, "v0.2.0", ""],
    ["", "v0.2.0", ASSET],
    ["goble", "v0.2.0", ASSET],
    ["goble-dev/goble/extra", "v0.2.0", ASSET],
    ["../goble", "v0.2.0", ASSET],
    [REPO, "v0.2.0", undefined],
  ];
  for (const [repo, tag, asset] of attempts) {
    assert.equal(downloadTarget(repo, tag, asset), null, `${repo} ${tag} ${asset}`);
  }
});

function requestWith(headers) {
  return new Request("https://downloads.goble.dev/health", { headers });
}

test("a bearer token is read from the Authorization header", () => {
  assert.equal(bearerToken(requestWith({ authorization: "Bearer s3cret" })), "s3cret");
  assert.equal(bearerToken(requestWith({ authorization: "bearer s3cret" })), "s3cret");
  assert.equal(bearerToken(requestWith({ authorization: "Bearer   s3cret" })), "s3cret");
});

test("anything that is not a bearer token reads as null", () => {
  assert.equal(bearerToken(requestWith({})), null);
  assert.equal(bearerToken(requestWith({ authorization: "Basic s3cret" })), null);
  assert.equal(bearerToken(requestWith({ authorization: "Bearer " })), null);
  assert.equal(bearerToken(requestWith({ authorization: "s3cret" })), null);
});

test("token comparison accepts only an exact match", () => {
  assert.equal(tokenMatches("s3cret", "s3cret"), true);
  assert.equal(tokenMatches("s3cret", "s3creT"), false);
  assert.equal(tokenMatches("s3cret", "s3cretlonger"), false);
  assert.equal(tokenMatches("", ""), false);
  assert.equal(tokenMatches(null, "s3cret"), false);
  assert.equal(tokenMatches("s3cret", undefined), false);
});
