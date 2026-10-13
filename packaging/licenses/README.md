# Dependency license supplements

Some locked crates omit license files from their published packages. The files
here preserve notices from their upstream repositories at the commits recorded
in each crate's `.cargo_vcs_info.json`. `overrides.json` records their exact source
URLs and applicable crate versions. These notices keep their upstream licenses.

`dispatch` 0.2.0 declares MIT and credits Steven Sheldon in its package manifest but
does not supply a separate license file upstream. Its supplement includes that
attribution and the standard MIT text, explicitly labeled as a supplement.

`scripts/package_release.py` combines these supplements with license and notice
files shipped in the locked Cargo packages. Release archives also include the
project's MIT license, interface font notices and the AU wrapper's notices.
Updating dependencies requires reviewing any missing or changed notices before
publishing new downloads.
