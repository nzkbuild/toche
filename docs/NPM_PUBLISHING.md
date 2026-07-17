# Publishing Toche on npm

The npm package is a small, dependency-free launcher. During installation it
downloads the matching native Toche archive from the same-version GitHub Release,
verifies the published SHA-256 checksum, and exposes the binary as `toche`.

## First publication

1. Publish the GitHub Release first. The npm installer cannot work until the four
   native archives and their checksum files are publicly downloadable.
2. Confirm you are signed in as the owner of the `@nzkbuild` scope:

   ```shell
   npm whoami
   ```

   The command must print `nzkbuild`.
3. Check out the exact public release tag and verify both version files agree:

   ```shell
   git checkout v1.0.9
   node -p 'require("./package.json").version'
   cargo metadata --no-deps --format-version 1
   ```

4. Inspect the package contents and run the installer tests:

   ```shell
   npm run test:npm
   npm pack --dry-run
   ```

5. Sign in to the npm account that owns `@nzkbuild`, then publish the public
   scoped package:

   ```shell
   npm login
   npm publish --access public
   ```

6. Verify from a clean environment:

   ```shell
   npm install -g @nzkbuild/toche@1.0.9
   toche --version
   toche --help
   ```

Do not publish the npm package before the GitHub Release. Do not reuse a version
after npm accepts it; release a patch version instead.

## Supported targets

- Windows x64
- Linux x64
- macOS x64
- macOS arm64

The installer rejects unsupported platform and architecture pairs with a clear
error instead of downloading the wrong binary.

## Future automated publishing

After the first manual publication, configure npm trusted publishing for this
public GitHub repository. A dedicated GitHub Actions workflow can then publish
with short-lived OIDC credentials and npm provenance instead of a long-lived npm
token. Keep a human approval environment in front of that job.
