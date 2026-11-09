# RKP Mini

Minimal KernelSU module that issues an Android RKP keybox on **Qualcomm** devices
by driving the on-device KeyMint TA as a CSR signing oracle (Native V2, vendor
command `0x3122`). Target: a single Qualcomm reference device (arm64); other devices and firmware are untested.

> Scope: **only** Native V2 (`0x3122`). No V1, no AIDL V2, no TEE/boot-state spoofing,
> no WebUI, no auto-issue, no remote key deployment. See the original external design note.

## What it does

- `rkp-mini status` — read-only prereq + keybox check, updates module description.
- `rkp-mini issue --acknowledge` — generate P-256 key → fetch RKP challenge → sign
  CSR via KeyMint `0x3122` → `signCertificates` → validate chain → write keybox.

## Build

```sh
./build.sh
```

Produces `dist/rkp_mini-v0.1.0.zip`, installable from the KernelSU manager.

Requires the Android NDK (`$NDK`, default `~/Android/Sdk/ndk/30.0.16248370`) and
`rustup target add aarch64-linux-android`.

## Install & use

1. KernelSU manager → install `dist/rkp_mini-v0.1.0.zip` → reboot (or reopen module page).
2. Tap **Action** on the module, or run:

```sh
su -c '/data/adb/modules/rkp_mini/bin/rkp-mini issue --acknowledge'
```

3. Artifacts land in `/data/misc/keystore/rkp-mini/output` (`keybox.xml`, `cert-chain.pem`,
   `cert-chain.der`, `private-key.pem`, `meta.json`). Copy `keybox.xml` into your keystore
   module (e.g. TrickyStore). This module does not install it for you.

## go/no-go (reference device)

KeyMint 3 firmware may reject `0x3122`:

- `-100` UNIMPLEMENTED — TA lacks the command.
- `-18` INCOMPATIBLE_KEY_FORMAT — TA wants MacedPublicKey; Native V2 unavailable.
- `-40` INVALID_TAG — vendor command/frame mismatch.
- HTTP `444` from `signCertificates` — device not registered.

On any of these the module stops with a clear error; it never falls back to V1/AIDL.

## Extension points (not implemented)

- Scheduled auto-issue (`auto_issue_days` config, default 0 = off).
- Remote private-key deployment (`meta.json` already carries device id / chain digests).

## License

MIT. Portions modelled on Duck-ToolBox (MIT).
