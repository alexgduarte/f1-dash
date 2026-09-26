"use client";

import { useEffect } from "react";
import { useRouter } from "next/navigation";

import { isNative } from "@/lib/platform";

// Link handling inside the native app: external links go to the system
// browser, and internal links meant for a new tab (the app has no tabs) open
// in place.
export default function NativeBridge() {
	const router = useRouter();

	useEffect(() => {
		if (!isNative()) return;

		document.documentElement.dataset.native = "true";

		const onClick = (event: MouseEvent) => {
			if (event.defaultPrevented || event.button !== 0) return;
			if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;

			const anchor = (event.target as Element | null)?.closest?.("a[href]");
			if (!(anchor instanceof HTMLAnchorElement)) return;

			const url = new URL(anchor.href, window.location.href);

			if (url.origin === window.location.origin) {
				if (anchor.target === "_blank") {
					event.preventDefault();
					router.push(url.pathname + url.search + url.hash);
				}
				return;
			}

			event.preventDefault();
			import("@tauri-apps/plugin-opener")
				.then(({ openUrl }) => openUrl(url.href))
				.catch((error) => console.error("failed to open link", error));
		};

		// capture phase, so this runs before next/link's own handler
		document.addEventListener("click", onClick, true);
		return () => document.removeEventListener("click", onClick, true);
	}, [router]);

	return null;
}
