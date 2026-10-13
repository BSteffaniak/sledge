# Isolated candidate native build contract r5

Candidate only: `-DSLEDGE_STOCK_PRODUCTION -DSLEDGE_STOCK_CANDIDATE` plus quoted
string defines below (or generated header supplied with compiler `-include`):

- SLEDGE_STOCK_BUILD_ID: 64 lowercase hex SHA256 canonical package build contract
- SLEDGE_STOCK_ELF_SHA256: exact .Hyprland-wrapped ELF SHA256, not wrapper script
- SLEDGE_STOCK_SDK_API_HASH: exact SDK __hyprland_api_get_client_hash string
- SLEDGE_STOCK_MACHINE_ID_SHA256: SHA256 raw /etc/machine-id bytes, newline included
- SLEDGE_STOCK_BOOT_MARKER_PATH: absolute isolated guest marker path
- SLEDGE_STOCK_BOOT_MARKER_SHA256: SHA256 raw marker bytes

Dependency: OpenSSL development headers + libcrypto (`-lcrypto`), candidate only.
Hashing uses EVP SHA256; no subprocess, no handwritten hash implementation.
Startup gate runs before any hook installation. /proc/self/exe regular file is
bounded at128MiB. Scope guards use O_NOFOLLOW, root-owned regular files, no write
mode bits, max4096 bytes, digest exact bytes. In-read metadata changes reject.
These markers are test-package scoping guards, NOT cryptographic VM proof.

Candidate activates replacementWire in production route WITHOUT research mode,
fault/drop/trace env overrides. Explicit daemon endpoint remains
SLEDGE_HYPRLAND_PASSIVE_SOCKET; missing endpoint rejects startup. Hello caps stay0;
artifact `sledge-stock-v1:<build-id>` unchanged. Unknowncommit is accepted only
through exact compiled ELF+SDK pin and daemon independent descriptor/catalogue
verification. Normal production without CANDIDATE keeps replacementWire closed.
No descriptor caller Boolean enables capture.

Rust/CLI catalogue policy1 entry additionally binds descriptor_sha256,
evidence_sha256, feature_mask1, status=candidate and isolated_vm=true; descriptor
binds plugin file digest/path and actual compositor digest/path. Native does NOT
embed circular self-digest or parse caller descriptor; daemon checks plugin file
and mapped inode independently. Candidate is not host support and must never be
loaded into the active host desktop. Build/package must label it isolated-test.

Current known host ELF digest:
8dee6b387615f14e8ce6c1f7bbd0b58524c31d783061093423e6a3fc6f89b9ca
SDK: unknown_aq_0.9_hu_0.10_hg_0.4_hc_0.1_hlg_0.6
These alone do not grant qualification. Current scratch candidate-test.so uses
zero scope/build pins solely to prove compilation; it cannot qualify a guest.
Coordinator must generate actual VM scope pins and build ID for its real package.

Validation: six native tests, SHA256 known-vector/untrusted-scope rejection test,
18-detour structural check, passive/research syntax, research/normal-production/
candidate shared-object compiles. No VM or active desktop invocation here.
