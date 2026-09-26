import type { Viewport } from "next";

export const viewport: Viewport = {
	colorScheme: "dark",
	themeColor: "#09090b",
	initialScale: 1,
	maximumScale: 10,
	minimumScale: 0.1,
	userScalable: true,
	// draw under the notch and home indicator; layouts pad with the safe-area insets
	viewportFit: "cover",
};
