# S6 public verification anchors

These files are public Ed25519 SPKI PEM verification anchors only; neither is
a private key or a signing capability.

- `kleidophylax-release-authority.pub` is the exact Kleidophylax S6 release
  authority public PEM, SHA-256
  `sha256:5352d77b634ea113b7869af3d514765263fe23dba7a094624f43b8c43f3e31e4`.
- `mnemosyne-expedition-s3-ed25519-v1.pub` is the distinct Expedition S3
  attestation authority public PEM, SHA-256
  `sha256:28c20216a58da0307b6303cbd76940c5a7ba6f5469adb389a85dd4290c46f546`.

Their separation follows the authoritative Programme S6 custody contract at
`94248526d066a1082fb3b21ad5e0b4905c53827f` and the source-only
Kleidophylax S6 signer contract in PR #11. They are used only to fail closed
before typed S0--S5 and Jenkins evidence validation becomes available.
