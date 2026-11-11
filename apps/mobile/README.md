# Goble Mobile

Expo (React Native + TypeScript) client for a Goble gateway. Android and iOS
run the same TypeScript shell; platform capabilities live in native modules
written in Kotlin and Swift.

## Run

```bash
npm install
npm start
```

Use `npm run android`, `npm run ios` or `npm run web` for a target. Without a
configured gateway the app opens a demo workspace, so the home screen is usable
before the server exists.

## Connect a gateway

Open the status pill in the home header, enter a gateway URL, and save. The app
calls `GET /health` and then `GET /api/v1/home`. The endpoint contract lives in
`../server` once the control plane lands; until then a live server can return the
same shape as `lib/mock.ts`.

## Structure

| Path | Purpose |
|---|---|
| `app/` | expo-router screens and layouts |
| `components/` | native-feeling UI pieces |
| `lib/` | theme, API, demo data, formatting |
| `modules/` | Expo native modules (Kotlin + Swift) |
| `plugins/` | Expo config plugins for permissions and entitlements |
| `.maestro/` | device smoke flows |
| `assets/` | app icon and splash |
