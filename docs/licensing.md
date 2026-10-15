# Licensing

Lullaby is now developed privately for a commercial release. The root
[LICENSE](../LICENSE) reserves rights in project-owned unpublished changes and
future project-owned work. Both Rust packages disable publishing to a package
registry and refer to that license file.

## Previously published version

The published 0.1.2 baseline at commit
`baa8463921a319fe6f37cae224286ec4142fa750` was distributed under MIT. Its original
notice is retained byte-for-byte in
[licenses/MIT-BASELINE.txt](../licenses/MIT-BASELINE.txt).

Existing MIT grants remain in force for the material they cover. Repository
visibility and this licensing change do not withdraw previously granted rights,
including redistribution and commercial use of that material. Preserve the MIT
notice when distributing copies or substantial portions of the covered code.
Any other existing license grants also remain unaffected. Proprietary terms
cannot override rights in code owned or separately licensed by others.

## Dependencies and assets

Dependencies retain their licenses. Embedded fonts retain their copyright and
license notices in [assets/fonts](../assets/fonts/README.md). Commercial release
packaging must include the project license, the MIT baseline notice and all
applicable third-party notices. The maintained notice generator verifies the full
locked dependency graph against Cargo's source archive checksums and pinned
upstream notice files. Bundles and [beta archives](beta-builds.md) include those
notices and their dependency manifest; AU bundles also include native-wrapper
notices.

Uncleared recordings, design reference images and investigation material are
excluded from the maintained source tree and distributions. Private records are
retained separately for review; the Git history is preserved. This cleanup does
not change the development history or establish independent legal clearance.

## Commercial release

The owner is arranging an offline intellectual-property review. Customer terms
and any rights assignments or contributor permissions needed for commercial
distribution must be established separately. This document records the source
licensing boundary; it is not a customer EULA or a license to redistribute
private changes.

Marketing, checkout and customer binary delivery should be hosted separately
from the private source repository. Existing downloads do not receive a new
license retroactively. No replacement storefront or customer delivery service
has been published as part of this source transition.
