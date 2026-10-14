# Translations

LightFoundry ships in English only, with no translation or font files (the assets rule). You can
supply your own.

A translation is a JSON file:

```json
{
  "name": "Deutsch",
  "font": "/path/to/a/font/that/has/your/script.ttf",
  "strings": { "Open…": "Öffnen…", "Rulers": "Lineale" }
}
```

- `strings` maps the English text, exactly as the program shows it, to your text. Anything missing
  or empty stays English.
- Texts with values in them are keyed with named places, for example `"Job finished: {lines} lines"`;
  keep the `{name}` places in your translation, in whatever order your language needs. A place you
  leave out simply is not shown; one that is not recognised is left as written.
- `font` is optional. The built-in font covers Latin scripts; for others (Cyrillic extras, Greek
  polytonic, CJK, Arabic, …) name a `.ttf` or `.otf` that has the characters. It is added behind the
  built-in one.
- Use **View > Language > Save translation template…** to write every text the program has shown so
  far, each as its own translation, then edit the values. Open the panels and menus you want covered
  before saving; the template lists only what has been drawn.
- **View > Language > Load translation…** picks a file, keeps a copy in the data folder and uses it
  from then on. **Use English** removes it.

Limits: 4 MB per file, 20,000 strings, 2,000 characters per text, 32 MB per font.

## What is covered

Menus, buttons, labels, tooltips, headings, drop-downs, and the status-bar messages the desktop app
writes itself (including the ones with values in them). Not covered yet: messages returned by the
engine (errors and command results, which can appear in the status bar), a few small texts such as
layer descriptions built from layer names and durations like "1m 05s", the CLI, and the browser
build (which has no way to pick a file).
