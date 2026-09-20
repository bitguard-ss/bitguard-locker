import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { nodePolyfills } from "vite-plugin-node-polyfills";

// Ledger connects over WebHID, which browsers only allow in a "secure context".
// http://localhost counts as secure, so plain `vite dev` on localhost is fine —
// no HTTPS certificate needed for this to work.
//
// nodePolyfills() injects a real Buffer/global/process implementation into
// the browser bundle. Several Solana/Ledger libraries assume these exist
// (as they would in Node), and without this plugin you'll hit errors like
// "Buffer is not defined" at runtime.
export default defineConfig({
  plugins: [
    react(),
    nodePolyfills({
      globals: {
        Buffer: true,
        global: true,
        process: true,
      },
    }),
  ],
  server: {
    host: "localhost",
    port: 5173,
    strictPort: true,
  },
});
