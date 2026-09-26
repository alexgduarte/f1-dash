import type { NextConfig } from "next";

import pack from "./package.json" with { type: "json" };

import "@/env";

// NEXT_EXPORT=1 builds a fully static site into ./out. The native app (Tauri)
// bundles that output, and it can also be hosted on any static file server.
// The Tauri CLI sets TAURI_ENV_* for the builds it drives.
const exportBuild = process.env.NEXT_EXPORT === "1" || !!process.env.TAURI_ENV_PLATFORM;

const output = exportBuild ? "export" : process.env.NEXT_STANDALONE === "1" ? "standalone" : undefined;
const compress = process.env.NEXT_NO_COMPRESS === "1";

const frameDisableHeaders = [
	{
		source: "/(.*)",
		headers: [
			{
				type: "header",
				key: "X-Frame-Options",
				value: "SAMEORIGIN",
			},
			{
				type: "header",
				key: "Content-Security-Policy",
				value: "frame-ancestors 'self';",
			},
		],
	},
];

const config: NextConfig = {
	output,
	compress,
	// every route becomes <route>/index.html, which the native app's asset
	// resolver and plain static hosts both serve without rewrites
	trailingSlash: exportBuild,
	env: {
		version: pack.version,
	},
	images: {
		unoptimized: exportBuild,
		remotePatterns: [
			{
				protocol: "https",
				hostname: "**formula1.com",
				port: "",
			},
		],
	},
	...(exportBuild ? {} : { headers: async () => frameDisableHeaders }),
};

export default config;
