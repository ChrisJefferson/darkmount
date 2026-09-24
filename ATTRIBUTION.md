# Attribution and protocol provenance

We used the Dark Mount QLink framing, command identities and device-specific
protocol documentation from the GPL-3.0-only project
[`re133/iocenter-linux`](https://github.com/re133/iocenter-linux), inspected at
commit `6e7a10a27fe5d2e552dec9d7c6adb0ba17191da9`, as guidance.

The firmware compatibility boundary and DirectDraw research were checked
against [`JimStroomberg/Dorkmount-patcher`](https://github.com/JimStroomberg/Dorkmount-patcher),
inspected at commit `e8c06ef37abc88f1b251cc4e230c93e316e43985`.

We independently exercised the official Windows application while capturing
its USB traffic. These captures verified the documented behaviour against a
connected model 1, hardware revision 1 Dark Mount keyboard and identified the
requests used to write assignments, settings and images. A native
IOHIDManager probe independently checked the macOS transport behaviour.

No source code from either project is copied or translated into this
repository. Their licences continue to govern their source code; this project's
independently written source code is licensed under MPL-2.0.
