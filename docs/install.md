# Install Lullaby beta

Choose the download for your computer, then extract the ZIP. Close your DAW
before installing or replacing a plugin. Keep the extracted README, notices and
manifest together for reference.

Lullaby does not run by itself: open it as an audio effect in your DAW. Install
one format your DAW supports; you do not need all the formats.

## Windows x64

Copy the whole **Lullaby.vst3** folder to:

`C:\Program Files\Common Files\VST3\`

For CLAP, copy **Lullaby.clap** together with **FONT-LICENSES.txt**,
**LULLABY-LICENSES.txt**, **THIRD-PARTY-NOTICES.txt** and
**DEPENDENCY-MANIFEST.json** to:

`C:\Program Files\Common Files\CLAP\`

Windows may ask for administrator permission to copy into these locations.
The beta is not publisher signed. Reopen the DAW and rescan its plugins.

## macOS: Intel and Apple Silicon

In Finder, choose **Go → Go to Folder…** and enter the appropriate path:

| Format | Folder |
| --- | --- |
| VST3 | `~/Library/Audio/Plug-Ins/VST3/` |
| CLAP | `~/Library/Audio/Plug-Ins/CLAP/` |
| Audio Unit | `~/Library/Audio/Plug-Ins/Components/` |

Create the folder if necessary. Copy the whole **Lullaby.vst3**,
**Lullaby.clap** or **Lullaby.component** bundle into it. Preserve its internal
contents. The universal package works with both processor architectures.

The initial beta has an ad-hoc signature and is not Apple notarized. If macOS
blocks a plugin you obtained from the owner, review the message and, where
available, use **System Settings → Privacy & Security → Open Anyway** to approve
that specific item. You may need to reopen the DAW to trigger the approval
prompt. If approval is unavailable or validation still fails, send the message
to the beta contact. Do not disable Gatekeeper globally or change security
settings for unrelated software.

Restart the DAW and rescan or reset its plugin cache if required. Logic uses the
Audio Unit version. Avoid having another Lullaby copy in the system-wide
`/Library/Audio/Plug-Ins/` folders at the same time.

## Linux x64

Copy the whole **Lullaby.vst3** folder into `~/.vst3/`.
For CLAP, copy **Lullaby.clap**, **FONT-LICENSES.txt**, **LULLABY-LICENSES.txt**,
**THIRD-PARTY-NOTICES.txt** and **DEPENDENCY-MANIFEST.json** into `~/.clap/`.
Create the folders if necessary.

Use an archive extractor that preserves Unix file permissions. If the plugin
is not found, confirm that your DAW scans these locations, then restart and
rescan. Send any missing-library error to the beta contact along with your
Linux distribution and version.

## Check the loaded version

Open Lullaby and check its version in **Processing**. Compare it with the download
filename and manifest. If an older version appears, look for duplicate copies in
other plugin folders and restart the DAW after removing the duplicate.

Existing Lullaby plugin identifiers are retained so DAWs can recognize saved
instances. This does not guarantee that every beta change will sound identical
in an existing session; keep a backup while testing.

## Roll back or uninstall

Close the DAW. Remove the beta plugin files or bundles from the folders above.
To roll back, put your saved previous version back in the same location, restart
the DAW and rescan. Open a backup of the project if you saved it while testing a
newer version. Removing the plugin does not remove your DAW projects.

## Optional download checksum

Compare the ZIP's SHA-256 with **SHA256SUMS** or its companion **.sha256** file.
Download the checksum from the same trusted beta delivery channel.

- Windows PowerShell: `Get-FileHash .\Lullaby-<version>-Windows-x86_64.zip -Algorithm SHA256`
- macOS Terminal: `shasum -a 256 Lullaby-<version>-macOS-universal.zip`
- Linux Terminal: `sha256sum Lullaby-<version>-Linux-x86_64.zip`

Replace `<version>` with the version in your download's filename. A matching
checksum detects corruption; it does not replace trusting the download source.
