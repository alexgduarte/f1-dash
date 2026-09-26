"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { utc } from "moment";

import type { ArchiveMeeting, ArchiveSession } from "@/types/replay.type";

import { fetchReplaySessions } from "@/lib/datasource";
import { useReplayStore } from "@/stores/useReplayStore";
import { useSettingsStore } from "@/stores/useSettingsStore";

import SegmentedControls from "@/components/ui/SegmentedControls";

// the live timing archive starts in 2018
const FIRST_YEAR = 2018;

type Listing = { year: number; meetings: ArchiveMeeting[] | null; error: boolean };

export default function ReplayPage() {
	const router = useRouter();
	const start = useReplayStore((state) => state.start);
	const setSeries = useSettingsStore((state) => state.setSeries);

	const currentYear = new Date().getFullYear();
	const years = Array.from({ length: 4 }, (_, i) => currentYear - i).filter((y) => y >= FIRST_YEAR);

	const [year, setYear] = useState<number>(currentYear);
	const [listing, setListing] = useState<Listing | null>(null);

	useEffect(() => {
		let cancelled = false;

		fetchReplaySessions(year)
			.then((meetings) => !cancelled && setListing({ year, meetings, error: false }))
			.catch((error) => {
				console.error("failed to list replays", error);
				if (!cancelled) setListing({ year, meetings: null, error: true });
			});

		return () => {
			cancelled = true;
		};
	}, [year]);

	const current = listing?.year === year ? listing : null;

	const play = (meeting: ArchiveMeeting, session: ArchiveSession) => {
		// replays come from the Formula 1 archive
		setSeries("f1");
		start({ path: session.path, name: `${meeting.name} · ${session.name}` });
		router.push("/dashboard");
	};

	return (
		<div className="flex flex-col gap-4 p-2">
			<div className="flex flex-wrap items-end justify-between gap-2">
				<div>
					<h1 className="text-3xl">Replay</h1>
					<p className="text-zinc-500">
						Watch a past Formula 1 session again with its timing, tyres, race control and team radio.
					</p>
				</div>

				<SegmentedControls
					id="replay-year"
					options={years.map((y) => ({ label: String(y), value: y }))}
					selected={year}
					onSelect={setYear}
				/>
			</div>

			{!current && (
				<div className="grid grid-cols-1 gap-2 md:grid-cols-2">
					{new Array(6).fill(null).map((_, i) => (
						<div key={i} className="h-28 animate-pulse rounded-lg bg-zinc-800" />
					))}
				</div>
			)}

			{current?.error && <p className="text-zinc-400">The session archive could not be loaded.</p>}

			{current?.meetings && current.meetings.length === 0 && (
				<p className="text-zinc-400">No finished sessions in {year} yet.</p>
			)}

			{current?.meetings && current.meetings.length > 0 && (
				<div className="grid grid-cols-1 gap-2 md:grid-cols-2">
					{[...current.meetings].reverse().map((meeting) => (
						<div key={meeting.key} className="rounded-lg border border-zinc-800 p-3">
							<div className="mb-2 flex items-baseline justify-between gap-2">
								<h2 className="font-semibold">{meeting.name}</h2>
								<p className="text-sm text-zinc-500">{meeting.location}</p>
							</div>

							<div className="flex flex-wrap gap-1.5">
								{meeting.sessions.map((session) => (
									<button
										key={session.key}
										onClick={() => play(meeting, session)}
										title={utc(session.start).format("ddd D MMM YYYY, HH:mm") + " local time"}
										className="cursor-pointer rounded-md bg-zinc-800 px-2 py-1 text-sm hover:bg-zinc-700"
									>
										{session.name}
									</button>
								))}
							</div>
						</div>
					))}
				</div>
			)}
		</div>
	);
}
