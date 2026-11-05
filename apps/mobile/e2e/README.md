# Mobile smoke tests

The first flow is `../.maestro/smoke.yaml`. It boots the app, checks the home
screen, and visits the Runs and Settings tabs.

```bash
# Start an Android emulator or iOS simulator first.
npm run test:e2e
```

Later flows will cover gateway pairing, a live run, and an approval. Keep
credentials in the invocation environment; never commit an account or a server
token.
