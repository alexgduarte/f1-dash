import type { Rainviewer } from "@/types/rainviewer.type";

const rainviewerUrl = "https://api.rainviewer.com/public/weather-maps.json";

export const getRainviewer = async (): Promise<Rainviewer | null> => {
	try {
		const response = await fetch(rainviewerUrl);

		if (!response.ok) {
			return null;
		}

		return response.json();
	} catch {
		return null;
	}
};

/** Radar frames oldest first; the forecast (nowcast) part is not always offered. */
export const getRadarFrames = (rainviewer: Rainviewer) => {
	const past = rainviewer.radar?.past ?? [];
	const nowcast = rainviewer.radar?.nowcast ?? [];

	return {
		frames: [...past, ...nowcast],
		latestPast: Math.max(past.length - 1, 0),
	};
};
