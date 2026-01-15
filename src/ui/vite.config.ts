import vue from "@vitejs/plugin-vue"
import UnoCSS from "unocss/vite"
import { defineConfig } from "vite"
import Vuetify, { transformAssetUrls } from "vite-plugin-vuetify"
import fs from "fs"
import path from "path"

// https://vite.dev/config/
export default defineConfig({
	plugins: [
		vue({
			template: { transformAssetUrls }
		}),
		Vuetify(),
		UnoCSS()
	],
	css: {
		preprocessorOptions: {
			sass: {
				api: "modern-compiler"
			}
		}
	},
    preview: process.env.VITE_HTTPS === 'true' ? {
        port: 5173,
        https: {
            key: fs.readFileSync(path.resolve(__dirname, '../../temp/certs/server.key.pem')),
            cert: fs.readFileSync(path.resolve(__dirname, '../../temp/certs/server.cert.pem')),
        }
    } : {
        port: 5173
    }
})
