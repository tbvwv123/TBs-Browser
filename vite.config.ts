import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// @fontsource의 @font-face는 woff2와 옛 woff를 함께 적는다. WebView2는 woff2만 쓰므로 woff 쪽을 지워
// 설치 파일에 woff 파일이 들어가지 않게 한다. @import를 펼친 뒤에 도는 PostCSS 단계에서 고친다.
const woff2Only = {
  postcssPlugin: "tbs-woff2-only",
  Declaration: {
    src(decl: { value: string }) {
      decl.value = decl.value.replace(/,\s*url\([^)]*\.woff\)\s*format\(['"]woff['"]\)/g, "");
    },
  },
};

// UI 번들은 dist/에 만들고, Tauri가 빌드할 때 앱 안에 넣는다.
export default defineConfig({
  plugins: [svelte()],
  css: { postcss: { plugins: [woff2Only] } },
  clearScreen: false,
  build: { outDir: "dist", emptyOutDir: true, target: "es2022" },
});
