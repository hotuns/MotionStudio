import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
export default defineConfig({ plugins: [react({jsxImportSource:'@motion-i18n'})], resolve:{alias:{'@motion-i18n':new URL('./src/i18n-runtime',import.meta.url).pathname.replace(/^\/([A-Za-z]:)/,'$1')}}, clearScreen: false, server: { host: '127.0.0.1', port: 1435, strictPort: true, watch: { ignored: ['**/target/**', '**/vendor/**', '**/src-tauri/**'] } }, build: { target: 'es2022' } });
