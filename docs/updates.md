# Windows updates

EdgeBridge v0.2.0 and later check for a newer release on startup. v0.3.0 also checks every six hours while open. They download the installer from GitHub Releases and verify the bundled public key's signature and the signed version before showing **Install update**. An active sender or receiver session must stop before installation. Each PC updates independently. On Windows, the updater installer waits briefly for the old process before performing the normal application and file-lock checks.

To migrate from v0.1.0, close EdgeBridge and run the v0.2.0 setup for the current Windows user on both PCs. Use the existing installation directory. After an app restart, a secondary PC generates a new pairing code when the receiver starts. The installer preserves edge preferences.

## Publish a Windows update

1. Set the same new version in Cargo.toml, tauri.conf.json and the root npm manifest. Update both lockfiles. Refresh the Windows dependency notices when dependencies or the release toolchain change.
2. Run the Windows workflow at the release commit. Download its x64 NSIS artifact and verify its provenance.
3. Sign that exact setup file with the offline Tauri updater key and the version from the built configuration. Use `tauri signer sign --app-version <version> --private-key-path <path> <setup.exe>`. Supply the signing password out of band. The application build and the hosted CI need only the public key.
4. Attach the setup file, its signature and `latest.json` to the same GitHub Release. The manifest uses `version` and `platforms.windows-x86_64` with `url` and `signature`. The signature is the contents of the .sig file. Upload the assets to a draft and publish the release as GitHub's Latest once the assets are verified.
5. Verify the public manifest and compare the downloaded installer hash. Test the update from an installed previous version before relying on unattended discovery of that release.

Treat the private key and its password as release credentials. Keep an encrypted backup. Replacing the public key in a binary already installed requires a fresh installation. The updater signature protects package integrity and version. Windows Authenticode determines the publisher shown by Windows installer warnings.

The installer includes a third-party inventory and exact upstream source locations. See [THIRD_PARTY_NOTICES](third-party-notices.txt) and the [Rust runtime notices](licenses/rust-library.html).
