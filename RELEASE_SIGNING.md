# Release signing and automatic updates

CypherLinks uses the Tauri 2 updater. Updater packages are generated with `bundle.createUpdaterArtifacts: true` and Tauri verifies release signatures before installation.

## One-time signing key setup

Generate the signing key on a secure release workstation:

```bash
npm run tauri signer generate -- -w ~/.tauri/cypherlinks.key
```

Store the private key and password in the release platform's encrypted secret store. Never commit the private key. Copy only the generated public key into the production updater configuration.

## Production updater configuration

Create `src-tauri/tauri.prod.conf.json` from `src-tauri/tauri.prod.conf.example.json`, then replace `CYPHERLINKS_PUBLIC_KEY` and the HTTPS update endpoint with the release infrastructure values. Build production releases with:

```bash
TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/cypherlinks.key)" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$CYPHERLINKS_SIGNING_PASSWORD" \
npm run tauri build -- --config src-tauri/tauri.prod.conf.json
```

The update endpoint must return Tauri-compatible release metadata and signatures. The application never accepts unsigned updater payloads.
