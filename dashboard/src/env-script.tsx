import { connection } from "next/server";
import Script from "next/script";

import { PUBLIC_ENV_KEY } from "@/env";

// only list env vars that can be exposed to the client

export const getPublicEnv = () => ({
	NEXT_PUBLIC_LIVE_URL: process.env.NEXT_PUBLIC_LIVE_URL,
	// API_URL is accepted for deployments configured before the schedule was fetched client side
	NEXT_PUBLIC_API_URL: process.env.NEXT_PUBLIC_API_URL ?? process.env.API_URL,
});

export default async function EnvScript() {
	// runtime injection needs a request; static exports keep the values present at build time
	if (process.env.NEXT_EXPORT !== "1" && !process.env.TAURI_ENV_PLATFORM) {
		await connection();
	}

	const env = getPublicEnv();

	const innerHTML = {
		__html: `window['${PUBLIC_ENV_KEY}'] = ${JSON.stringify(env)}`,
	};

	return <Script id="public-env" strategy={"beforeInteractive"} dangerouslySetInnerHTML={innerHTML} />;
}
