# Third-party notices

WebDAV Drive uses or interoperates with the following third-party projects. Each project remains subject to its own license.

## rclone

- Project: <https://github.com/rclone/rclone>
- Role: bundled mount engine and RC API
- License: MIT
- Copyright: Copyright (C) 2012 by Nick Craig-Wood <http://www.craig-wood.com/nick/>

The following MIT license notice is reproduced from rclone's `COPYING` file:

> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in
> all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
> THE SOFTWARE.

## WinFsp

- Project: <https://github.com/winfsp/winfsp>
- Role: external Windows file-system runtime used by rclone mount
- License: GPLv3 with a special exception for Free/Libre and Open Source Software; commercial licensing is also available
- Distribution: not bundled with WebDAV Drive; users install WinFsp separately

See WinFsp's authoritative `License.txt` for the complete terms: <https://github.com/winfsp/winfsp/blob/master/License.txt>.

## Tauri

- Project: <https://github.com/tauri-apps/tauri>
- Role: desktop host, WebView window, IPC, and system tray
- License: MIT or Apache-2.0 where applicable
- Copyright: Copyright (c) 2015-present, The Tauri Programme within The Commons Conservancy

See Tauri's license files for the complete terms: <https://github.com/tauri-apps/tauri/tree/dev#licenses>.

## Rust dependencies

Additional Rust libraries are declared in `Cargo.toml` and locked in `Cargo.lock`. Their source distributions and license metadata are retained by Cargo's registry cache and remain subject to their respective upstream terms.
