# AUR packages

- `rusty-inbox/`: builds the tagged release from source against the system Qt 6.
- `rusty-inbox-bin/`: repackages the prebuilt Linux tarball from GitHub Releases (system Qt 6).

Release flow: tag `vX.Y.Z`, set `pkgver`, `updpkgsums`, `makepkg --printsrcinfo > .SRCINFO`,
then commit both files to `ssh://aur@aur.archlinux.org/<pkgname>.git`.
