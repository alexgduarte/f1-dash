import { buildParams } from "@/lib/params";

import type { Coords, Place } from "@/types/geocode.type";

const CACHE_KEY = "geocode-cache";

// Circuits don't move, and Nominatim's usage policy asks clients to cache and
// keep to one request per second, so results are kept in local storage.
const readCache = (): Record<string, Coords | null> => {
	try {
		return JSON.parse(localStorage.getItem(CACHE_KEY) ?? "{}");
	} catch {
		return {};
	}
};

const writeCache = (query: string, coords: Coords | null) => {
	try {
		localStorage.setItem(CACHE_KEY, JSON.stringify({ ...readCache(), [query]: coords }));
	} catch {
		// storage can be unavailable (private mode, quota); caching is best effort
	}
};

export const fetchCoords = async (query: string): Promise<Coords | null> => {
	const cache = readCache();
	if (query in cache) return cache[query];

	const params = buildParams({
		q: query,
		format: "jsonv2",
	});

	try {
		const response = await fetch(`https://nominatim.openstreetmap.org/search${params}`);
		if (!response.ok) return null;

		const data: Place[] = await response.json();

		const best = data.sort((a, b) => b.importance - a.importance)[0];
		const coords = best ? { lon: parseFloat(best.lon), lat: parseFloat(best.lat) } : null;

		writeCache(query, coords);
		return coords;
	} catch {
		return null;
	}
};

/** Finds a circuit on the map, trying a few phrasings one after the other. */
export const locateCircuit = async (country: string | undefined, location: string): Promise<Coords | null> => {
	const place = country ? `${country}, ${location}` : location;

	for (const query of [`${place} circuit`, `${place} autodrome`, place]) {
		const coords = await fetchCoords(query);
		if (coords) return coords;
	}

	return null;
};
