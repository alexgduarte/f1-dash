"use client";

import { useEffect, useState } from "react";

import { getSeries, type SeriesId } from "@/lib/series";

type Props = {
	connected: boolean;
	series: SeriesId;
	replaying: boolean;
};

const GRACE_MS = 8000;

// Explains an empty dashboard once the feed has been unreachable for a while,
// instead of loading placeholders forever.
export default function FeedOffline({ connected, series, replaying }: Props) {
	const [offline, setOffline] = useState(false);

	useEffect(() => {
		if (connected) return;

		const timer = setTimeout(() => setOffline(true), GRACE_MS);
		return () => {
			clearTimeout(timer);
			setOffline(false);
		};
	}, [connected, series, replaying]);

	if (connected || !offline) return null;

	return (
		<div className="border-b border-amber-900/60 bg-amber-950/40 p-2 text-sm text-amber-300 md:rounded-lg md:border">
			{replaying
				? "The replay could not be loaded from the session archive. Retrying…"
				: `No connection to the ${getSeries(series).name} timing feed. It may be between sessions or unreachable; retrying…`}
		</div>
	);
}
