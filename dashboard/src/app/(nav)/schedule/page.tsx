"use client";

import { useSettingsStore } from "@/stores/useSettingsStore";
import { useSchedule } from "@/hooks/useSchedule";
import { getSeries } from "@/lib/series";

import NextRound from "@/components/schedule/NextRound";
import Schedule from "@/components/schedule/Schedule";
import SeriesPicker from "@/components/SeriesPicker";

export default function SchedulePage() {
	const series = useSettingsStore((state) => state.series);
	const setSeries = useSettingsStore((state) => state.setSeries);

	const { schedule, next, loading, error } = useSchedule(series);

	return (
		<div>
			<div className="my-4 flex flex-wrap items-end justify-between gap-2">
				<div>
					<h1 className="text-3xl">Up Next</h1>
					<p className="text-zinc-500">{getSeries(series).name} · all times are local time</p>
				</div>

				<SeriesPicker selected={series} onSelect={setSeries} layoutId="schedule-series" />
			</div>

			{loading ? <NextRoundLoading /> : <NextRound next={next} />}

			<div className="my-4">
				<h1 className="text-3xl">Schedule</h1>
				<p className="text-zinc-500">All times are local time</p>
			</div>

			{loading ? (
				<FullScheduleLoading />
			) : error ? (
				<div className="flex h-44 flex-col items-center justify-center">
					<p>Failed to load the schedule</p>
				</div>
			) : (
				<Schedule schedule={schedule} next={next} />
			)}
		</div>
	);
}

const RoundLoading = () => {
	return (
		<div className="flex flex-col gap-1">
			<div className="h-12 w-full animate-pulse rounded-md bg-zinc-800" />

			<div className="grid grid-cols-3 gap-8 pt-1">
				{Array.from({ length: 3 }).map((_, i) => (
					<div key={`day.${i}`} className="grid grid-rows-2 gap-2">
						<div className="h-12 w-full animate-pulse rounded-md bg-zinc-800" />
						<div className="h-12 w-full animate-pulse rounded-md bg-zinc-800" />
					</div>
				))}
			</div>
		</div>
	);
};

const NextRoundLoading = () => {
	return (
		<div className="grid h-44 grid-cols-1 gap-8 sm:grid-cols-2">
			<div className="flex flex-col gap-4">
				<div className="h-1/2 w-3/4 animate-pulse rounded-md bg-zinc-800" />
				<div className="h-1/2 w-3/4 animate-pulse rounded-md bg-zinc-800" />
			</div>

			<RoundLoading />
		</div>
	);
};

const FullScheduleLoading = () => {
	return (
		<div className="mb-20 grid grid-cols-1 gap-8 md:grid-cols-2">
			{Array.from({ length: 6 }).map((_, i) => (
				<RoundLoading key={`round.${i}`} />
			))}
		</div>
	);
};
