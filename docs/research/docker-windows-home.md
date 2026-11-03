# Does Docker Desktop run on Windows Home?

Ticket: kamui/rakazo-stack#20. All pages read on 2026-11-02. Docs lookup only; nothing was installed or run, and no Windows Home machine was tested.

## Answer

- **Technically yes, officially unclear.** Docker Desktop's WSL 2 backend rests on WSL 2, and Microsoft states WSL 2 is available on Windows 10 Home and Windows 11 Home. Docker's current install page no longer lists Home in the WSL 2 requirements (only Enterprise, Pro, Education), but the same page still says "Windows Home or Education editions only allow you to run Linux containers", and Docker's troubleshooting page still has a "WSL 2 and Windows Home" section. No Docker page says Home is unsupported or blocked.
- **Home was listed until October 2025.** Up to Docker docs commit `a2fc5c2b5` (2025-10-10) the WSL 2 tab read "Windows 11 64-bit: Home or Pro version 22H2 or higher ..." and "Windows 10 64-bit: Minimum required is Home or Pro 22H2 (build 19045) or higher ...". Commit `9661be249` (2025-10-23, PR docker/docs#23584) rewrote those lines. Its description only says it raises the minimum Windows version to 23H2 and follows Microsoft's end-of-servicing dates; it does not mention dropping Home. Treat the Home removal as undocumented, probably a side effect (an inference, not stated by Docker).
- **Minimum builds in Docker's current docs (for the listed editions):** Windows 10 22H2 (build 19045); Windows 11 23H2 (build 22631) or higher; WSL 2.1.5 or later (2.6+ for Enhanced Container Isolation); 64-bit CPU with SLAT; 8 GB RAM; the Windows Server service (LanmanServer) set to Automatic. Docker also says Desktop is supported only on Windows versions still in Microsoft's servicing timeline. Microsoft's own WSL 2 floor is lower (Windows 10 x64 1903, build 18362.1049) and Docker's WSL 2.1.5 floor sits above it.
- **Hyper-V backend: not for Home.** Docker lists the Hyper-V backend only for Enterprise, Pro and Education, and only with an all-users install. Home users get WSL 2 only. Docker's 4.5.0 release note says Home users cannot start Docker Desktop on the Hyper-V backend because "WSL 2 is the only supported backend".
- **Admin rights.** Enabling WSL 2 for the first time needs administrator rights (one-time, per machine): Microsoft's `wsl --install` runs from an elevated PowerShell, and Docker says the same. A per-user Docker Desktop install and updates need no admin; an all-users install does. Virtualization must be enabled in BIOS/UEFI, which is a firmware setting rather than a Windows admin right, and Docker notes many machines already have it on.
- **Documented alternative for Home:** none. Docker's pages give no Home-specific alternative and no "upgrade to Pro" guidance. (Docker Engine inside WSL 2 is out of scope for v1 per the map.)

## Details

### Docker, current requirements

Source: <https://docs.docker.com/desktop/setup/install/windows-install/> (read 2026-11-02; the page shows no last-updated date, its source file last changed 2026-10-26 in `docker/docs` commit `abab46c4d`).

- WSL 2 backend, x86_64: "WSL version 2.1.5 or later", "Windows 10 64-bit: Enterprise, Pro, or Education version 22H2 (build 19045)", "Windows 11 64-bit: Enterprise, Pro, or Education version 23H2 (build 22631) or higher", "Turn on the WSL 2 feature on Windows", SLAT CPU, 8 GB RAM, "Enable hardware virtualization in BIOS/UEFI".
- Hyper-V backend, x86_64: same editions and builds, plus "Turn on Hyper-V and Containers Windows features"; "Hyper-V is only available with all-users installation."
- Both tabs: "Windows Home or Education editions only allow you to run Linux containers." That sentence implies Home is a Docker Desktop host, which sits oddly beside the edition list. The page offers no reconciliation.
- Install modes: per-user "does not require administrator privileges to install or update"; all-users "Required". "Enabling WSL 2 for the first time also requires administrator privileges, but this is a one-time, per-machine operation."

### Docker, history of the Home line

- Source: `docker/docs` file `content/manuals/desktop/setup/install/windows-install.md`, compared at commits `a2fc5c2b5` (Home listed for WSL 2) and `9661be249` (removed). Diff: <https://github.com/docker/docs/commit/9661be249>; PR <https://github.com/docker/docs/pull/23584>; later `c7611bf42` (2025-10-24, "Add back support for Windows 10") restored the Windows 10 22H2 line, again without Home.
- Release notes (<https://docs.docker.com/desktop/release-notes/>, read 2026-11-02): 4.5.0/4.5.1 (Feb 2022) say Windows Home users "will not be able to start Docker Desktop as WSL 2 is the only supported backend"; a later fix stopped "Switch to Windows Containers" appearing in the tray on Windows Home editions. No release note announces dropping Home.
- Troubleshooting (<https://docs.docker.com/desktop/troubleshoot-and-support/troubleshoot/topics/>, source last changed 2026-10-20): section "WSL 2 and Windows Home" lists what must be present: Virtual Machine Platform, Windows Subsystem for Linux, virtualization enabled in the BIOS, and the hypervisor enabled at Windows startup; `wsl -l -v` must run without error.

### Microsoft

- <https://learn.microsoft.com/en-us/windows/wsl/faq> (page `ms.date` 2024-07-16, content updated 2026-07-30; read 2026-11-02): "Is WSL 2 available on Windows 10 Home and Windows 11 Home? Yes. WSL 2 is available on all Desktop SKUs where WSL is available, including Windows 10 Home and Windows 11 Home." It needs "Virtual Machine Platform" (a subset of Hyper-V) and "Windows Subsystem for Linux"; the Virtual Machine Platform component is "available on all Desktop SKUs".
- <https://learn.microsoft.com/en-us/windows/wsl/install> (`ms.date` 2025-06-09, updated 2026-06-02): prerequisite Windows 10 version 2004 (build 19041) or higher, or Windows 11; "Open PowerShell in administrator mode ... enter the wsl --install command, then restart your machine."
- <https://learn.microsoft.com/en-us/windows/wsl/install-manual> (`ms.date` 2026-04-15): WSL 2 on x64 needs Windows 10 version 1903 or later with build 18362.1049 or later, or Windows 11; enabling features runs in PowerShell "as Administrator"; Virtual Machine Platform "will require virtualization capabilities".
- <https://learn.microsoft.com/en-us/windows/wsl/troubleshooting> (`ms.date` 2026-04-15): "make sure that virtualization is enabled inside of your computer's BIOS", and error 0x80370102 asks to enable the Virtual Machine Platform feature and BIOS virtualization.

## Open questions / follow-ups

- Is Home deliberately unsupported by Docker since 2025-10-23, or an accidental docs omission? Docker docs and release notes do not say. A question to Docker (docs issue) would settle it; until then rakazo-stack can only claim "Home is not listed".
- Does the current Docker Desktop install and run on a Windows 11 Home machine with WSL 2 (and does the installer or app refuse)? Only a test on a real Home machine answers this (suggested type: task).
- Decision for the spec: on Windows Home, should preflight hard-stop, or warn that Docker does not list Home and continue if `docker` works (suggested type: grilling)?
- Windows 11 23H2 Home/Pro is past, or near, Microsoft end of servicing; Docker ties support to the servicing timeline. The exact dates were not checked here (**unverified**) and affect which Home builds Docker would still cover.

## Sources

- <https://docs.docker.com/desktop/setup/install/windows-install/>
- <https://docs.docker.com/desktop/release-notes/>
- <https://docs.docker.com/desktop/troubleshoot-and-support/troubleshoot/topics/>
- <https://github.com/docker/docs/commit/9661be249> and <https://github.com/docker/docs/pull/23584> (docs history)
- <https://learn.microsoft.com/en-us/windows/wsl/faq>
- <https://learn.microsoft.com/en-us/windows/wsl/install>
- <https://learn.microsoft.com/en-us/windows/wsl/install-manual>
- <https://learn.microsoft.com/en-us/windows/wsl/troubleshooting>
