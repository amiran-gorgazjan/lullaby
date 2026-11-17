# Lullaby beta

Lullaby is an adaptive resonance suppressor. It helps soften harsh frequencies
while preserving the character of a sound.

Version 0.2.2 reduces processing CPU use, with about 41% less processing time for
the default preset in local macOS VST3 tests against 0.2.1. Results vary by setup.
Quality, latency, controls and saved parameter IDs are unchanged.

This is a beta build. Keep your previous version available and
save a copy of an important project before trying the beta.

## What's included

- Windows x64: VST3 and CLAP.
- macOS Intel and Apple Silicon: universal VST3, CLAP and Audio Unit (AU).
- Linux x64: VST3 and CLAP.

Each download contains the formats for its named platform. See **INSTALL.md**
for installation and rollback instructions. The package's **manifest.json**
identifies the exact version, source revision, architectures and signing status.
These beta packages are not standalone applications.

## Signing and compatibility

The initial macOS beta uses an ad-hoc integrity signature and is **not Apple
notarized**. Windows builds are **not publisher signed**. Check the manifest for
the actual package's status. macOS may block the plugin when it is downloaded;
follow the instructions in INSTALL.md. Do not disable system security globally.

Automated build and format checks do not establish compatibility with every DAW
or operating-system version. Please report your OS, processor, DAW version,
plugin format and Lullaby version when giving feedback. An audio example and
steps to reproduce a problem are especially useful. Send reports through the
beta channel provided with your download.

## Package verification and notices

**SHA256SUMS** and the ZIP's companion **.sha256** file identify the exact download.
The package manifest includes hashes of every included file. Keep LICENSE,
MIT-BASELINE.txt, FONT-LICENSES.txt, LULLABY-LICENSES.txt and
THIRD-PARTY-NOTICES.txt and DEPENDENCY-MANIFEST.json with the package. The latter
records dependency source identities and notice hashes. AU-specific dependency notices are also
inside the component bundle.

LICENSE contains the MIT terms for Lullaby's project-owned code. Fonts and
third-party dependencies are covered by their accompanying notices.
