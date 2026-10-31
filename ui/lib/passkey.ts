// Passkeys for the lock (src/lock.rs): made and used by the window's WebAuthn
// (navigator.credentials); the backend makes the challenges and checks the signatures.

import { invoke } from "@tauri-apps/api/core";
import { _ } from "./i18n.svelte";

const b64 = (b: ArrayBuffer) => btoa(String.fromCharCode(...new Uint8Array(b)));
const b64url = (b: ArrayBuffer) => b64(b).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
const bytes = (s: string) => Uint8Array.from(atob(s.replaceAll("-", "+").replaceAll("_", "/")), (c) => c.charCodeAt(0));

/** This window can use passkeys (WebView2 can; WebKitGTK can't). */
export const passkeysWork = () => typeof PublicKeyCredential !== "undefined" && window.isSecureContext;

/** The browser's refusal in plain words: a cancelled or timed-out question says so. */
function plain(e: unknown): Error {
  return e instanceof DOMException && e.name === "NotAllowedError" ? new Error(_("The passkey wasn't used.")) : (e as Error);
}

/** The passkey's answer to a new challenge, as `lock_unlock` takes it. `passkey`: its id and relying party. */
export async function passkeyAnswer([id, rpId]: [string, string]) {
  const challenge = bytes(await invoke<string>("lock_challenge"));
  const publicKey = { challenge, rpId, allowCredentials: [{ type: "public-key" as const, id: bytes(id) }], userVerification: "required" as const, timeout: 60000 };
  const cred = (await navigator.credentials.get({ publicKey }).catch((e) => Promise.reject(plain(e)))) as PublicKeyCredential;
  const r = cred.response as AuthenticatorAssertionResponse;
  return { with: "passkey", auth: b64(r.authenticatorData), client: b64(r.clientDataJSON), sig: b64(r.signature) };
}

/** A new passkey for this computer, as `lock_set` takes it. */
export async function passkeyMake() {
  const challenge = bytes(await invoke<string>("lock_challenge"));
  const publicKey = {
    challenge,
    rp: { name: "xtermium", id: location.hostname },
    user: { id: crypto.getRandomValues(new Uint8Array(16)), name: "xtermium", displayName: "xtermium" },
    // ES256, EdDSA, RS256: what lock.rs checks.
    pubKeyCredParams: [-7, -8, -257].map((alg) => ({ type: "public-key" as const, alg })),
    authenticatorSelection: { userVerification: "required" as const, residentKey: "discouraged" as const },
    attestation: "none" as const,
    timeout: 60000,
  };
  const cred = (await navigator.credentials.create({ publicKey }).catch((e) => Promise.reject(plain(e)))) as PublicKeyCredential;
  const r = cred.response as AuthenticatorAttestationResponse;
  const key = r.getPublicKey();
  if (!key) throw new Error(_("This kind of passkey isn't supported."));
  return { id: b64url(cred.rawId), alg: r.getPublicKeyAlgorithm(), key: b64(key), client: b64(r.clientDataJSON) };
}
