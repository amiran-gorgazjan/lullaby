# Licensing

The [MIT License](../LICENSE) covers project-owned code and documentation in
the current Lullaby source tree, including the plugin, the DSP crate and the
build tooling. It permits use, modification and redistribution, including for
commercial purposes. Include the copyright and permission notice with copies
or substantial portions of the covered material.

## Previously published version

The published 0.1.2 baseline at commit
`baa8463921a319fe6f37cae224286ec4142fa750` was distributed under MIT. Its original
notice is retained byte-for-byte in
[licenses/MIT-BASELINE.txt](../licenses/MIT-BASELINE.txt).

Existing license grants continue to apply to the material they cover. Consult
the notices distributed with earlier releases for their terms.

## Dependencies and assets

Third-party dependencies and assets retain their own licenses. The embedded
fonts' copyright and license notices are in [assets/fonts](../assets/fonts/README.md).
Keep the project license, the MIT baseline notice and applicable third-party
notices with distributions. The notice generator verifies the locked dependency
graph against Cargo's source archive checksums and pinned upstream notice files.
Bundles and [beta archives](beta-builds.md) include these notices and the dependency
manifest. Audio Unit bundles also include native-wrapper notices.
