Pyodide runtime bundle for the secure-exec Python sidecar.

Bundled runtime files:
- `pyodide.mjs`
- `pyodide.asm.js`
- `pyodide.asm.wasm`
- `pyodide-lock.json`
- `python_stdlib.zip`

Bundled offline package wheels:
- `click-8.3.1-py3-none-any.whl`
- `micropip-0.11.0-py3-none-any.whl`
- `numpy-2.2.5-cp313-cp313-pyodide_2025_0_wasm32.whl`
- `pandas-2.3.3-cp313-cp313-pyodide_2025_0_wasm32.whl`
- `python_dateutil-2.9.0.post0-py2.py3-none-any.whl`
- `pytz-2025.2-py2.py3-none-any.whl`
- `six-1.17.0-py2.py3-none-any.whl`
- `cffi-1.17.1-cp313-cp313-pyodide_2025_0_wasm32.whl`
- `charset_normalizer-3.4.4-py3-none-any.whl`
- `cryptography-46.0.1-cp313-abi3-pyodide_2025_0_wasm32.whl`
- `et_xmlfile-2.0.0-py3-none-any.whl`
- `libopenssl-1.1.1w.zip`
- `lxml-6.0.2-cp313-cp313-pyodide_2025_0_wasm32.whl`
- `openpyxl-3.1.5-py2.py3-none-any.whl`
- `pdfminer_six-20250506-py3-none-any.whl`
- `pdfplumber-0.11.7-py3-none-any.whl`
- `pillow-11.3.0-cp313-cp313-pyodide_2025_0_wasm32.whl`
- `pycparser-2.22-py3-none-any.whl`
- `pypdf-6.13.2-py3-none-any.whl`
- `python_docx-1.1.2-py3-none-any.whl`
- `python_pptx-1.0.2-py3-none-any.whl`
- `reportlab-5.0.0-py3-none-any.whl`
- `typing_extensions-4.15.0-py3-none-any.whl`
- `xlsxwriter-3.2.9-py3-none-any.whl`

Bundle size as vendored in this directory:
- Core Pyodide runtime: 12,284,028 bytes
- Offline package wheels and shared libraries: 22,750,615 bytes
- Total: 35,034,643 bytes (33.41 MiB)

`python-runner.mjs` points `indexURL` at this local directory and defaults `packageBaseUrl` to the same bundled asset root so `pyodide.loadPackage()` and the built-in `micropip` bootstrap stay offline.

Before executing a non-interactive Python program, the runner calls Pyodide's
`loadPackagesFromImports()` against the program source. Only packages imported
by that invocation and their locked dependencies are loaded. This keeps plain
Python startup small while making the bundled data and document libraries
available without network access or an explicit install step.

Dynamic package installs:
- `AGENTOS_PYODIDE_PACKAGE_BASE_URL` can override the package base used by Pyodide package resolution when a Python execution needs to install additional wheels from a network-visible host.
- The bundled `micropip` wheel is still loaded from the local asset directory first so package-manager bootstrap does not depend on external network access.
- `await micropip.install("https://.../package.whl")` goes through the Python runner's bridge-backed fetch path, which means network permissions are enforced by the secure-exec kernel rather than bypassing it.

Debug timing output:
- Set `AGENTOS_PYTHON_WARMUP_DEBUG=1` on a Python execution request to emit `__AGENTOS_PYTHON_WARMUP_METRICS__:` JSON lines on stderr.
- The Rust execution engine emits a `phase:"prewarm"` line that reports whether warmup executed or reused the cached compile-cache path, plus the measured warmup duration in milliseconds.
- `python-runner.mjs` emits a `phase:"startup"` line just before guest code runs, including total startup time, `loadPyodide()` time, package-load time, package count, and whether the source was inline code, a file, or prewarm-only.

Startup targets:
- Cold start target: first request in a fresh cache should keep the combined prewarm plus startup path under `3000ms` on commodity hardware.
- Warm start target: cached follow-up requests should keep the `phase:"startup"` time under `500ms`.
