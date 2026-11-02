# App icon

`app-icon.png` is the user-supplied three-robot artwork with its white exterior
corners replaced by the matching dark background. Keep it square and full bleed;
Android and Google Play apply their own masks.

`play-icon.png` is the 512 × 512 RGB export for the Play listing. Generate the
native icon sizes from the master, from the repository root:

```powershell
cargo tauri icon mobile_rust/branding/app-icon.png --output mobile_rust/src-tauri/icons --ios-color '#1c1f2b'
```

The Tauri command also updates the generated Android launcher resources when
the Android project is present. Check both the ordinary launcher icon and the
adaptive icon foreground/background before shipping.
