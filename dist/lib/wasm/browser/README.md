# rq-library-wasm

This package provides a WebAssembly build of the `rq-library` for use in the browser.

## Installation

```bash
npm install rq-library-wasm
```

## Usage

```javascript
import init, { some_function } from 'rq-library-wasm';

async function run() {
  await init();
  some_function();
}

run();