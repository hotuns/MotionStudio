import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
// Read the JSX import source from tsconfig so the local runtime shares the app's live language state.
export default defineConfig(({mode})=>({
  plugins:[react()],
  resolve:{alias:{'@motion-i18n':new URL('./src/i18n-runtime',import.meta.url).pathname.replace(/^\/([A-Za-z]:)/,'$1')}},
  clearScreen:false,
  server:{host:'127.0.0.1',port:1435,strictPort:true,watch:{ignored:['**/target/**','**/vendor/**','**/src-tauri/**']}},
  // The OBS browser runs outside Tauri. Its local server uses the same built
  // assets in development and in the packaged app, without a Vite dependency.
  build:{target:'es2022',emptyOutDir:mode!=='obs-dev',
    rollupOptions:{input:mode==='obs-dev'?'obs.html':['index.html','obs.html']}},
}));
