import Image from "next/image";

import type { Stint } from "@/types/state.type";

type Props = {
	stints: Stint[] | undefined;
	/** the feed's own pit stop count, for feeds that only report the current tyre */
	pitStops?: number;
	/** the series publishes no tyre data at all */
	unavailable?: boolean;
};

export default function DriverTire({ stints, pitStops, unavailable }: Props) {
	if (unavailable) {
		return (
			<div className="flex flex-row items-center gap-2 place-self-start">
				<div className="flex h-8 w-8 items-center justify-center rounded-full border border-zinc-800 text-zinc-600">
					–
				</div>
				<div>
					<p className="leading-none font-medium text-zinc-600">L -</p>
					<p className="text-sm leading-none text-zinc-500">PIT {pitStops ?? "-"}</p>
				</div>
			</div>
		);
	}

	const stops = stints && stints.length > 1 ? stints.length - 1 : (pitStops ?? 0);
	const currentStint = stints ? stints[stints.length - 1] : null;
	const unknownCompound = !["soft", "medium", "hard", "intermediate", "wet"].includes(
		currentStint?.Compound?.toLowerCase() ?? "",
	);

	return (
		<div className="flex flex-row items-center gap-2 place-self-start">
			{currentStint && !unknownCompound && currentStint.Compound && (
				<Image
					src={"/tires/" + currentStint.Compound.toLowerCase() + ".svg"}
					width={32}
					height={32}
					alt={currentStint.Compound}
				/>
			)}

			{currentStint && unknownCompound && (
				<div className="flex h-8 w-8 items-center justify-center">
					<Image src={"/tires/unknown.svg"} width={32} height={32} alt={"unknown"} />
				</div>
			)}

			{!currentStint && <div className="h-8 w-8 animate-pulse rounded-full bg-zinc-800 font-semibold" />}

			<div>
				<p className="leading-none font-medium">
					L {currentStint?.TotalLaps ?? 0}
					{currentStint?.New === "false" ? "*" : ""}
				</p>

				<p className="text-sm leading-none text-zinc-500">PIT {stops}</p>
			</div>
		</div>
	);
}
