# Third-party notices

Husk Webcam is MIT licensed; see [LICENSE](LICENSE). That grant covers the code written for
this project. Two vendored components under `src/HuskFilter/` carry their own copyright, and
both arrived through Unity Capture, which vendors them the same way. Both are MIT, so the whole
of this repository is distributable under MIT terms -- but the copyright holders are not the
same, and their notices must not be removed or rewritten.

All line counts below were measured on 2026-09-21 with `wc -l` and `grep -c` over the files as
they stand in this repository. The method is named beside each number, because a count without
its command is a claim rather than a measurement.

---

## 1. Unity Capture

**Files:** `src/HuskFilter/HuskFilter.cpp`, `HuskFilter.def`, `shared.inl` (1.430 lines)

```
Unity Capture
Copyright (c) 2018 Bernhard Schelling
Based on UnityCam, Copyright (c) 2016 MHD Yamen Saraiji
The MIT License (MIT)
```

Upstream: <https://github.com/schellingb/UnityCapture>

The full notice is reproduced verbatim at the top of each of those files.

**Vendored with three changes, all of them naming**, so that this filter and an unmodified
Unity Capture can be installed side by side without reaching each other's clients:

1. the capture device name (`HuskFilter.cpp`)
2. the four CLSIDs (`HuskFilter.cpp`)
3. the four shared kernel object names -- mutex, two events and the file mapping (`shared.inl`)

Change 2 without change 3 would be half a rebranding: two filters with separate COM
registrations still sharing one piece of memory.

---

## 2. Microsoft DirectShow base classes

**Files:** `src/HuskFilter/streams.h`, `streams.cpp`

```
Copyright (c) 1992-2001 Microsoft Corporation.  All rights reserved.
```

These two files are Microsoft's DirectShow base classes -- the `baseclasses` sample that ships
with the Windows SDK. Microsoft publishes that sample at
<https://github.com/microsoft/Windows-classic-samples> under
`Samples/Win7Samples/multimedia/directshow/baseclasses/`, and **that repository's `LICENSE` is
the MIT License, Copyright (c) Microsoft Corporation.**

They are used unmodified.

### What was measured, and what was not

- `streams.h` + `streams.cpp` are **17.603 of the 19.033 lines** of source under
  `src/HuskFilter/` -- 92,5 % (`wc -l` over `*.h *.cpp *.inl *.def`; the build script
  `byg.ps1` is this project's own and is not counted).
- They carry **31 Microsoft copyright notices**
  (`grep -c 'Copyright (c).*Microsoft'`: 10 in `streams.h`, 21 in `streams.cpp`). 28 of those
  read `1992-2001`, two `1995-2001` and one `1996-2001`.
- `streams.cpp` is an **amalgamation of 21 of the individual base-class source files**
  (`grep -c '^// File:'`), each still carrying its own banner: `DlleEntry.cpp`, `WXDebug.cpp`,
  `AMFilter.cpp`, `Source.cpp`, `DllSetup.cpp` and so on.
- `streams.h`'s header is byte-identical to Microsoft's own apart from the case of the file
  name in the `// File:` banner (`Streams.h` upstream, `streams.h` here).

⚠️ **The amalgamation has NOT been diffed file by file against Microsoft's originals.** What is
measured is the provenance and the licence of the sample these files come from, not that every
one of the 21 concatenated sources is byte-identical to its upstream counterpart. If that
matters to you, diff them yourself; the upstream files are at the URL above.
