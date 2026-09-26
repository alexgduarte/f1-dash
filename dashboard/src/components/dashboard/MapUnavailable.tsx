import { getSeries } from "@/lib/series";
import { useSettingsStore } from "@/stores/useSettingsStore";

export default function MapUnavailable() {
	const series = useSettingsStore((state) => state.series);

	return (
		<div className="flex h-full min-h-60 flex-col items-center justify-center gap-1 rounded-lg border border-zinc-800 p-4 text-center">
			<p className="font-medium">No track map for {getSeries(series).name}</p>
			<p className="max-w-md text-sm text-zinc-500">
				Car positions come from mini-sector timing, which only the Formula 1 feed publishes.
			</p>
		</div>
	);
}
