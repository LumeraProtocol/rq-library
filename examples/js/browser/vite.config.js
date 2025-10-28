import { defineConfig } from 'vite';
import { viteStaticCopy } from 'vite-plugin-static-copy';

export default defineConfig({
  plugins: [
    viteStaticCopy({
      targets: [
        {
          src: 'node_modules/rq-library-wasm/rq_library_bg.wasm',
          dest: '.'
        }
      ]
    })
  ]
});