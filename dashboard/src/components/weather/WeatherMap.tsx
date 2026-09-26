"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import clsx from "clsx";

import maplibregl, { type Map, Marker } from "maplibre-gl";
import "maplibre-gl/dist/maplibre-gl.css";

import { locateCircuit } from "@/lib/geocode";
import { getRadarFrames, getRainviewer } from "@/lib/rainviewer";
import { getWindDirection } from "@/lib/getWindDirection";

import { useDataStore } from "@/stores/useDataStore";

import PlayControls from "@/components/ui/PlayControls";
import RadarTimeline from "@/components/weather/RadarTimeline";

const RADAR_OPACITY = 0.8;

type Frame = { id: number; time: number };

type Props = {
	/** smaller overlays for use as a dashboard panel */
	compact?: boolean;
};

// an arrow pointing downwind; the feed reports where the wind comes from
const createWindMarker = () => {
	const element = document.createElement("div");
	element.innerHTML = `<svg width="44" height="44" viewBox="0 0 44 44" aria-hidden="true">
		<circle cx="22" cy="22" r="20" fill="rgba(9,9,11,0.75)" stroke="rgba(96,165,250,0.9)" stroke-width="2"/>
		<path d="M22 8 L29 26 L22 22 L15 26 Z" fill="#60a5fa"/>
	</svg>`;
	element.style.pointerEvents = "none";

	return new Marker({ element, rotationAlignment: "map" });
};

export default function WeatherMap({ compact = false }: Props) {
	const location = useDataStore((state) => state.state?.SessionInfo?.Meeting?.Location);
	const country = useDataStore((state) => state.state?.SessionInfo?.Meeting?.Country?.Name);
	const weather = useDataStore((state) => state.state?.WeatherData);

	const mapContainerRef = useRef<HTMLDivElement>(null);
	const mapRef = useRef<Map | null>(null);
	const windMarkerRef = useRef<Marker | null>(null);
	const currentFrameRef = useRef<number>(0);

	const [status, setStatus] = useState<"loading" | "ready" | "no-location">("loading");
	const [radarUnavailable, setRadarUnavailable] = useState<boolean>(false);
	const [playing, setPlaying] = useState<boolean>(false);
	const [frames, setFrames] = useState<Frame[]>([]);
	const [initialFrame, setInitialFrame] = useState<number>(0);

	useEffect(() => {
		if (!mapContainerRef.current || !location) return;

		let cancelled = false;
		let map: Map | null = null;

		(async () => {
			const coords = await locateCircuit(country, location);
			if (cancelled || !mapContainerRef.current) return;

			if (!coords) {
				setStatus("no-location");
				return;
			}

			map = new maplibregl.Map({
				container: mapContainerRef.current,
				style: "https://basemaps.cartocdn.com/gl/dark-matter-gl-style/style.json",
				center: [coords.lon, coords.lat],
				zoom: compact ? 8 : 9,
				attributionControl: { compact: true },
				canvasContextAttributes: {
					antialias: true,
				},
			});

			mapRef.current = map;

			map.on("load", async () => {
				if (cancelled || !map) return;

				setStatus("ready");

				new Marker({ color: "#e11d48" }).setLngLat([coords.lon, coords.lat]).addTo(map);
				windMarkerRef.current = createWindMarker().setLngLat([coords.lon, coords.lat]).addTo(map);

				const rainviewer = await getRainviewer();
				if (cancelled || !map) return;

				if (!rainviewer) {
					setRadarUnavailable(true);
					return;
				}

				const radar = getRadarFrames(rainviewer);

				radar.frames.forEach((frame, i) => {
					map?.addLayer({
						id: `rainviewer-frame-${i}`,
						type: "raster",
						source: {
							type: "raster",
							tiles: [`${rainviewer.host}${frame.path}/256/{z}/{x}/{y}/8/1_0.webp`],
							tileSize: 512,
							maxzoom: 6,
							minzoom: 0,
							volatile: false,
							attribution: '<a href="https://www.rainviewer.com/" target="_blank">RainViewer</a>',
						},
						paint: {
							"raster-opacity": i === radar.latestPast ? RADAR_OPACITY : 0,
							"raster-fade-duration": 200,
							"raster-resampling": "nearest",
						},
					});
				});

				currentFrameRef.current = radar.latestPast;
				setInitialFrame(radar.latestPast);
				setFrames(radar.frames.map((frame, i) => ({ time: frame.time, id: i })));
			});
		})();

		return () => {
			cancelled = true;
			map?.remove();
			mapRef.current = null;
			windMarkerRef.current = null;
			setFrames([]);
			setStatus("loading");
			setRadarUnavailable(false);
		};
	}, [location, country, compact]);

	const windDeg = weather ? parseFloat(weather.WindDirection) : NaN;

	useEffect(() => {
		if (!Number.isNaN(windDeg)) windMarkerRef.current?.setRotation(windDeg + 180);
	}, [windDeg, status]);

	const setFrame = useCallback((idx: number) => {
		const map = mapRef.current;
		if (!map || !map.getLayer(`rainviewer-frame-${idx}`)) return;

		map.setPaintProperty(`rainviewer-frame-${currentFrameRef.current}`, "raster-opacity", 0);
		map.setPaintProperty(`rainviewer-frame-${idx}`, "raster-opacity", RADAR_OPACITY);
		currentFrameRef.current = idx;
	}, []);

	return (
		<div className="relative h-full w-full overflow-hidden rounded-lg">
			<div ref={mapContainerRef} className="absolute h-full w-full" />

			{weather && status === "ready" && (
				<div
					className={clsx(
						"absolute top-0 left-0 z-10 m-2 grid grid-cols-3 gap-x-4 gap-y-1 rounded-lg bg-black/80 backdrop-blur-xs",
						compact ? "p-2 text-xs" : "p-3 text-sm",
					)}
				>
					<Reading label="Track" value={`${Math.round(parseFloat(weather.TrackTemp))}°`} />
					<Reading label="Air" value={`${Math.round(parseFloat(weather.AirTemp))}°`} />
					<Reading label="Humidity" value={`${Math.round(parseFloat(weather.Humidity))}%`} />
					<Reading
						label="Wind"
						value={`${parseFloat(weather.WindSpeed)} m/s ${Number.isNaN(windDeg) ? "" : getWindDirection(windDeg)}`}
					/>
					<Reading label="Rain" value={weather.Rainfall === "1" ? "Yes" : "No"} highlight={weather.Rainfall === "1"} />
					{weather.Pressure && <Reading label="Pressure" value={`${Math.round(parseFloat(weather.Pressure))} hPa`} />}
				</div>
			)}

			{status === "ready" && frames.length > 0 && (
				<div
					className={clsx(
						"absolute right-0 bottom-0 left-0 z-20 m-2 flex gap-4 rounded-lg bg-black/80 backdrop-blur-xs md:right-auto",
						compact ? "p-2 md:w-md" : "p-4 md:w-lg",
					)}
				>
					<PlayControls playing={playing} onClick={() => setPlaying((v) => !v)} />

					<RadarTimeline frames={frames} setFrame={setFrame} playing={playing} initialFrame={initialFrame} />
				</div>
			)}

			{radarUnavailable && (
				<p className="absolute right-0 bottom-0 left-0 z-20 m-2 rounded-lg bg-black/80 p-2 text-sm text-zinc-400">
					Rain radar is unavailable right now.
				</p>
			)}

			{status === "no-location" && (
				<div className="flex h-full w-full items-center justify-center text-sm text-zinc-500">
					Could not find this circuit on the map.
				</div>
			)}

			{(status === "loading" || !location) && <div className="absolute inset-0 animate-pulse rounded-lg bg-zinc-800" />}
		</div>
	);
}

function Reading({ label, value, highlight }: { label: string; value: string; highlight?: boolean }) {
	return (
		<div className="flex flex-col">
			<span className="text-[10px] tracking-wide text-zinc-500 uppercase">{label}</span>
			<span className={clsx("font-semibold tabular-nums", highlight ? "text-sky-400" : "text-white")}>{value}</span>
		</div>
	);
}
