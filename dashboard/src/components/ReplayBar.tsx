"use client";

import { useEffect, useRef, useState } from "react";

import { useDataStore } from "@/stores/useDataStore";
import { useReplayStore } from "@/stores/useReplayStore";

import PlayControls from "@/components/ui/PlayControls";

const SPEEDS = [0.5, 1, 2, 4, 8, 16, 32];

// how much of the build-up before the start the scrubber offers
const LEAD_IN_MS = 10 * 60 * 1000;

/** Session time relative to lights out, e.g. "-00:45" or "1:12:03". */
const formatSessionTime = (ms: number) => {
	const sign = ms < 0 ? "-" : "";
	const total = Math.floor(Math.abs(ms) / 1000);
	const hours = Math.floor(total / 3600);
	const minutes = Math.floor((total % 3600) / 60)
		.toString()
		.padStart(2, "0");
	const seconds = (total % 60).toString().padStart(2, "0");

	return hours > 0 ? `${sign}${hours}:${minutes}:${seconds}` : `${sign}${minutes}:${seconds}`;
};

type Props = {
	name: string;
};

export default function ReplayBar({ name }: Props) {
	const status = useDataStore((state) => state.state?.Replay);
	const request = useReplayStore((state) => state.request);
	const control = useReplayStore((state) => state.control);
	const stop = useReplayStore((state) => state.stop);

	// the scrubber position while it is being dragged
	const [scrubbing, setScrubbing] = useState<number | null>(null);

	const position = scrubbing ?? status?.Position ?? 0;
	const start = status?.Start ?? 0;
	const end = status?.End ?? 0;
	const min = Math.max(0, Math.min(start - LEAD_IN_MS, position));

	// the server reports the position once a second; between reports it is
	// estimated, so pausing or changing speed does not jump back
	const reportRef = useRef<{ position: number; at: number } | null>(null);

	useEffect(() => {
		if (status) reportRef.current = { position: status.Position, at: Date.now() };
	}, [status]);

	const currentPosition = () => {
		const report = reportRef.current;
		if (!status || !report) return status?.Position;
		if (status.Paused || status.Ended) return status.Position;
		return Math.min(end, report.position + (Date.now() - report.at) * status.Speed);
	};

	const seek = (to: number) => {
		setScrubbing(null);
		control({ from: to });
	};

	const togglePlay = () => {
		// a finished replay starts over
		if (status?.Ended) control({ from: min, paused: false });
		else control({ from: currentPosition(), paused: !request.paused });
	};

	return (
		<div className="flex flex-wrap items-center gap-3 border-b border-zinc-800 p-2 md:rounded-lg md:border">
			<div className="flex items-center gap-2">
				<span className="rounded-md bg-violet-600 px-2 py-0.5 text-xs font-bold tracking-wide uppercase">Replay</span>
				<p className="max-w-60 truncate text-sm font-medium">{name}</p>
			</div>

			<PlayControls playing={!request.paused && !status?.Ended} loading={!status} onClick={togglePlay} />

			<div className="flex min-w-48 flex-1 items-center gap-2">
				<span className="w-16 text-right text-sm text-zinc-400 tabular-nums">
					{formatSessionTime(position - start)}
				</span>

				<input
					type="range"
					aria-label="Replay position"
					className="flex-1 accent-violet-500"
					min={min}
					max={end}
					step={1000}
					value={position}
					disabled={!status}
					onChange={(e) => setScrubbing(Number(e.target.value))}
					onPointerUp={(e) => seek(Number(e.currentTarget.value))}
					onKeyUp={(e) => seek(Number(e.currentTarget.value))}
				/>

				<span className="w-16 text-sm text-zinc-500 tabular-nums">{formatSessionTime(end - start)}</span>
			</div>

			<label className="flex items-center gap-1 text-sm text-zinc-400">
				Speed
				<select
					className="rounded-md bg-zinc-800 px-1 py-0.5 text-white"
					value={request.speed}
					disabled={!status}
					onChange={(e) => control({ from: currentPosition(), speed: Number(e.target.value) })}
				>
					{SPEEDS.map((speed) => (
						<option key={speed} value={speed}>
							{speed}×
						</option>
					))}
				</select>
			</label>

			<button
				className="cursor-pointer rounded-md px-2 py-1 text-sm text-zinc-400 hover:bg-zinc-800 hover:text-white"
				onClick={stop}
			>
				Back to live
			</button>
		</div>
	);
}
