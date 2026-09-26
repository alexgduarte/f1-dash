export const SERIES = [
	{ id: "f1", name: "Formula 1", short: "F1" },
	{ id: "f2", name: "FIA Formula 2", short: "F2" },
	{ id: "f3", name: "FIA Formula 3", short: "F3" },
	{ id: "f1a", name: "F1 Academy", short: "F1A" },
	{ id: "wec", name: "FIA World Endurance Championship", short: "WEC" },
] as const;

export type SeriesId = (typeof SERIES)[number]["id"];

export const DEFAULT_SERIES: SeriesId = "f1";

export const isSeriesId = (value: unknown): value is SeriesId => SERIES.some((series) => series.id === value);

export const getSeries = (id: SeriesId) => SERIES.find((series) => series.id === id) ?? SERIES[0];
