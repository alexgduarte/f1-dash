"use client";

import { useSettingsStore } from "@/stores/useSettingsStore";

import LeaderBoard from "@/components/dashboard/LeaderBoard";
import RaceControl from "@/components/dashboard/RaceControl";
import TeamRadios from "@/components/dashboard/TeamRadios";
import TrackViolations from "@/components/dashboard/TrackViolations";
import TyreSets from "@/components/dashboard/TyreSets";
import Map from "@/components/dashboard/Map";
import MapUnavailable from "@/components/dashboard/MapUnavailable";
import WeatherMap from "@/components/weather/WeatherMap";
import Footer from "@/components/Footer";

export default function Page() {
	const series = useSettingsStore((state) => state.series);
	const f1 = series === "f1";

	return (
		<div className="flex w-full flex-col gap-2">
			<div className="flex w-full flex-col gap-2 2xl:flex-row">
				<div className="overflow-x-auto">
					<LeaderBoard />
				</div>

				<div className="flex-1 2xl:max-h-[50rem]">{f1 ? <Map /> : <MapUnavailable />}</div>
			</div>

			<div className="grid grid-cols-1 gap-2 divide-y divide-zinc-800 *:h-[30rem] *:overflow-y-auto *:rounded-lg *:border *:border-zinc-800 *:p-2 md:divide-y-0 lg:grid-cols-3">
				<div>
					<RaceControl />
				</div>

				{f1 && (
					<div>
						<TeamRadios />
					</div>
				)}

				<div>
					<TrackViolations />
				</div>
			</div>

			<div className="grid grid-cols-1 gap-2 lg:grid-cols-2">
				{f1 && (
					<div className="h-[34rem] overflow-y-auto rounded-lg border border-zinc-800 p-3">
						<TyreSets />
					</div>
				)}

				<div className="h-[34rem] overflow-hidden rounded-lg border border-zinc-800">
					<WeatherMap compact />
				</div>
			</div>

			<Footer />
		</div>
	);
}
