import clsx from "clsx";

import type { TimingDataDriver } from "@/types/state.type";

type Props = {
	timingDriver: TimingDataDriver;
	gridPos?: number;
	/** endurance racing: the car's class, shown with its class position */
	carClass?: string;
};

const classShort: Record<string, string> = {
	HYPERCAR: "HY",
	LMP2: "P2",
	LMGT3: "GT3",
};

const classColor: Record<string, string> = {
	HYPERCAR: "text-red-500",
	LMP2: "text-blue-500",
	LMGT3: "text-green-500",
};

export default function DriverInfo({ timingDriver, gridPos, carClass }: Props) {
	const positionChange = gridPos && gridPos - parseInt(timingDriver.Position);
	const gain = positionChange && positionChange > 0;
	const loss = positionChange && positionChange < 0;

	const status = timingDriver.KnockedOut
		? "OUT"
		: !!timingDriver.Cutoff
			? "CUTOFF"
			: timingDriver.Retired
				? "RETIRED"
				: timingDriver.Stopped
					? "STOPPED"
					: timingDriver.InPit
						? "PIT"
						: timingDriver.PitOut
							? "PIT OUT"
							: null;

	if (carClass) {
		return (
			<div className="place-self-start">
				<p className={clsx("text-lg leading-none font-medium tabular-nums", classColor[carClass] ?? "text-zinc-400")}>
					{classShort[carClass] ?? carClass} P{timingDriver.ClassPosition ?? "-"}
				</p>

				<p className="text-sm leading-none text-zinc-500">{status ?? "-"}</p>
			</div>
		);
	}

	return (
		<div className="place-self-start">
			<p
				className={clsx("text-lg leading-none font-medium tabular-nums", {
					"text-emerald-500": gain,
					"text-red-500": loss,
					"text-zinc-500": !gain && !loss,
				})}
			>
				{positionChange !== undefined
					? gain
						? `+${positionChange}`
						: loss
							? positionChange
							: "-"
					: `${timingDriver.NumberOfLaps}L`}
			</p>

			<p className="text-sm leading-none text-zinc-500">{status ?? "-"}</p>
		</div>
	);
}
