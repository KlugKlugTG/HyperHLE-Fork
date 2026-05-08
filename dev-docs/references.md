# References

A collection of external references useful for development, including
SDKs, emulators, ARM architecture documentation and PE format specs.

## Microsoft Windows CE / Windows Mobile SDKs and emulators

- Microsoft ARM Windows CE 6 emulator + random apps:
  <https://archive.org/details/win-ce-6-emulator>
- Microsoft Windows Mobile 2003 SE SDK with emulator:
  <https://archive.org/details/PocketPC2003SDK>
- Microsoft Windows Mobile 5 SDK with emulator:
  <https://archive.org/details/WindowsMobile5.0PocketPCSDKAndEmulator>
- Microsoft Windows Mobile 6.1 emulator:
  <https://archive.org/details/WM614Emulator>
- Microsoft Windows Mobile 6.5 Developer Tool Kit:
  <https://legacyupdate.net/download-center/download/17284/windows-mobile-6.5-developer-tool-kit>
- Microsoft x86 Windows CE 5 emulator:
  <https://archive.org/details/win-ce-5-emulator-fixed>
- Windows CE Platform SDK (H/PC) 2.0 (02/98):
  <https://archive.org/details/MPLATSDK.20>

## ARM architecture

- ARM Architecture Reference Manual (ARMv5TE):
  <https://developer.arm.com/documentation/ddi0100/latest>
- Thumb instruction set (PPC2003 code makes heavy use of Thumb):
  <https://developer.arm.com/documentation/ddi0210/latest>
- AAPCS calling convention for Windows CE (differs slightly from Linux EABI):
  <https://learn.microsoft.com/en-us/cpp/build/overview-of-arm-abi-conventions>

## PE format for ARM Windows CE

- PE/COFF specification (including `IMAGE_FILE_MACHINE_ARM` = `0x01C0`):
  <https://learn.microsoft.com/en-us/windows/win32/debug/pe-format>
