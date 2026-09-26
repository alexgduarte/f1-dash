import { useEffect, useState } from "react";

import type { Round } from "@/types/schedule.type";
import type { SeriesId } from "@/lib/series";

import { fetchSchedule } from "@/lib/datasource";

type ScheduleState = {
	schedule: Round[] | null;
	loading: boolean;
	error: boolean;
};

export const useSchedule = (series: SeriesId) => {
	const [state, setState] = useState<ScheduleState & { series: SeriesId | null }>({
		series: null,
		schedule: null,
		loading: true,
		error: false,
	});

	useEffect(() => {
		let cancelled = false;

		fetchSchedule(series)
			.then((schedule) => !cancelled && setState({ series, schedule, loading: false, error: false }))
			.catch((error) => {
				console.error("error fetching schedule", error);
				if (!cancelled) setState({ series, schedule: null, loading: false, error: true });
			});

		return () => {
			cancelled = true;
		};
	}, [series]);

	// a result for another series is stale while the new one loads
	const current = state.series === series;

	return {
		schedule: current ? state.schedule : null,
		loading: !current || state.loading,
		error: current && state.error,
		next: current ? (state.schedule?.find((round) => !round.over) ?? null) : null,
	};
};
