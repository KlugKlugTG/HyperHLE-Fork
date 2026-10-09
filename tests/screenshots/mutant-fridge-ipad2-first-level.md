# Mutant Fridge — iPad 2 first-level smoke run

- Profile: iPad 2, landscape-left; `UIScreen` reported 1024×768.
- Input: three taps through Play, Chapter 1, and the unlocked first level.
- Harness: `dev-scripts/ai-tap-sequence.py` with `--require-touch-delivery`.
- Result: **PASS** — emulator stayed alive, reported ongoing frames, and all 3 touch begin/end pairs reached UIKit. The final screenshot shows the first-level tutorial prompt.
- Caveat: the rendered game view occupies only the upper part of the iPad frame; the lower area is black. The log also contains non-fatal Objective-C/SQLite warnings, so this is not proof of clean full-screen rendering.
