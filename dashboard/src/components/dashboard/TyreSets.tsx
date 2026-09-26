"use client";

import { useState } from "react";
import clsx from "clsx";

import type { Compound, TyreSet } from "@/types/state.type";

import { useDataStore } from "@/stores/useDataStore";
import { useSettingsStore } from "@/stores/useSettingsStore";

import DriverTag from "@/components/driver/DriverTag";
import TyreBadge from "@/components/tyres/TyreBadge";
import Toggle from "@/components/ui/Toggle";

const compoundName: Record<Compound, string> = {
	SOFT: "Soft",
	MEDIUM: "Medium",
	HARD: "Hard",
	INTERMEDIATE: "Intermediate",
	WET: "Wet",
};

const isSlick = (compound: Compound) => compound === "SOFT" || compound === "MEDIUM" || compound === "HARD";

const describe = (set: TyreSet): string => {
	const parts = [compoundName[set.Compound], set.New ? "new" : `used, ${set.Laps} laps`];

	if (set.Sessions.length > 0) parts.push(`run in ${set.Sessions.join(", ")}`);
	if (set.Fitted) parts.push("on the car now");
	if (set.ReturnedAfter) {
		parts.push(`handed back after ${set.ReturnedAfter}${set.ReturnEstimated ? " (estimated)" : ""}`);
	}

	return parts.join(" · ");
};

export default function TyreSets() {
	const tyreSets = useDataStore((state) => state.state?.TyreSets);
	const drivers = useDataStore((state) => state.state?.DriverList);
	const timing = useDataStore((state) => state.state?.TimingData);
	const sessionName = useDataStore((state) => state.state?.SessionInfo?.Name);
	const series = useSettingsStore((state) => state.series);

	const [showReturned, setShowReturned] = useState(false);
	const [showWet, setShowWet] = useState(false);

	if (series !== "f1") {
		return (
			<Empty
				title="Tyre sets"
				message="Tyre set tracking needs per-session tyre data, which only the Formula 1 feed provides."
			/>
		);
	}

	if (!tyreSets || !drivers) {
		return <Loading />;
	}

	// after qualifying the remaining sets are what a driver has for the race
	const forRace = sessionName === "Race" || tyreSets.Sessions.some((s) => s.Name === "Qualifying");

	const order = Object.keys(tyreSets.Lines).sort((a, b) => {
		const pa = parseInt(timing?.Lines[a]?.Position ?? "") || drivers[a]?.Line || 99;
		const pb = parseInt(timing?.Lines[b]?.Position ?? "") || drivers[b]?.Line || 99;
		return pa - pb;
	});

	const counted = tyreSets.Sessions.map((s) => s.Name).join(", ");
	const missing = tyreSets.Sessions.filter((s) => !s.Loaded).map((s) => s.Name);

	return (
		<div className="flex flex-col gap-2">
			<div className="flex flex-wrap items-start justify-between gap-2">
				<div>
					<h2 className="text-lg font-semibold">{forRace ? "Tyre sets for the race" : "Tyre sets remaining"}</h2>
					<p className="text-xs text-zinc-500">
						{tyreSets.Format === "sprint" ? "Sprint" : "Standard"} weekend · counts {counted || "this session"}
					</p>
				</div>

				<div className="flex flex-col gap-1 text-xs text-zinc-400">
					<label className="flex items-center justify-end gap-2">
						Rain tyres
						<Toggle enabled={showWet} setEnabled={setShowWet} />
					</label>
					<label className="flex items-center justify-end gap-2">
						Returned sets
						<Toggle enabled={showReturned} setEnabled={setShowReturned} />
					</label>
				</div>
			</div>

			<p className="text-xs text-zinc-500">
				Estimated. The regulations fix how many sets go back to Pirelli after each session, but teams pick most of them
				and the feed does not say which, so the most worn sets are assumed to go first.
			</p>

			{!tyreSets.HistoryLoaded && (
				<p className="rounded-md bg-amber-950/60 p-2 text-xs text-amber-300">
					Earlier sessions of this weekend could not be loaded, so only this session is counted.
				</p>
			)}

			{missing.length > 0 && (
				<p className="rounded-md bg-amber-950/60 p-2 text-xs text-amber-300">
					No timing data for {missing.join(", ")}; sets used there are only partly accounted for.
				</p>
			)}

			<div className="flex flex-col divide-y divide-zinc-900">
				{order.map((nr) => {
					const driver = drivers[nr];
					if (!driver) return null;

					const sets = tyreSets.Lines[nr].Sets.filter(
						(set) => (showReturned || !set.ReturnedAfter) && (showWet || isSlick(set.Compound)),
					);
					const available = tyreSets.Lines[nr].Sets.filter((set) => !set.ReturnedAfter && isSlick(set.Compound));
					const fresh = available.filter((set) => set.New).length;
					const expected = tyreSets.Lines[nr].ExpectedSlickSets;
					const mismatch = expected !== null && expected !== undefined && expected !== available.length;

					return (
						<div key={nr} className="flex items-center gap-3 py-1.5">
							<DriverTag className="w-[4.5rem] shrink-0" short={driver.Tla} teamColor={driver.TeamColour} />

							<div className="flex w-14 shrink-0 flex-col text-xs leading-tight">
								<span
									className={clsx("font-semibold", mismatch ? "text-amber-400" : "text-white")}
									title={
										mismatch
											? `The return rules leave ${expected} sets; the timing data does not add up for this driver`
											: undefined
									}
								>
									{available.length} sets{mismatch ? " !" : ""}
								</span>
								<span className="text-zinc-500">{fresh} new</span>
							</div>

							<div className="flex flex-wrap items-start gap-1.5">
								{sets.map((set, i) => (
									<TyreBadge
										key={`${nr}.${i}`}
										compound={set.Compound}
										label={set.New ? "NEW" : `${set.Laps}`}
										dimmed={!!set.ReturnedAfter}
										highlighted={set.Fitted}
										title={describe(set)}
									/>
								))}
							</div>
						</div>
					);
				})}
			</div>
		</div>
	);
}

function Empty({ title, message }: { title: string; message: string }) {
	return (
		<div className="flex h-full flex-col gap-1">
			<h2 className="text-lg font-semibold">{title}</h2>
			<p className="text-sm text-zinc-500">{message}</p>
		</div>
	);
}

function Loading() {
	return (
		<div className="flex flex-col gap-2">
			<div className="h-6 w-48 animate-pulse rounded-md bg-zinc-800" />
			{new Array(8).fill(null).map((_, i) => (
				<div key={i} className="flex items-center gap-3">
					<div className="h-7 w-[4.5rem] animate-pulse rounded-lg bg-zinc-800" />
					<div className={clsx("h-6 animate-pulse rounded-md bg-zinc-800", i % 2 ? "w-40" : "w-52")} />
				</div>
			))}
		</div>
	);
}
