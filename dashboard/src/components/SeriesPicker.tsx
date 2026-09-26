"use client";

import clsx from "clsx";
import { motion } from "motion/react";

import { SERIES, type SeriesId } from "@/lib/series";

type Props = {
	selected: SeriesId;
	onSelect: (series: SeriesId) => void;
	className?: string;
	layoutId?: string;
};

export default function SeriesPicker({ selected, onSelect, className, layoutId = "series-picker" }: Props) {
	return (
		<div role="tablist" aria-label="Championship" className={clsx("flex flex-wrap gap-0.5", className)}>
			{SERIES.map((series) => {
				const active = series.id === selected;

				return (
					<button
						key={series.id}
						role="tab"
						aria-selected={active}
						title={series.name}
						onClick={() => onSelect(series.id)}
						className={clsx(
							"relative cursor-pointer rounded-md px-1.5 py-1 text-sm font-semibold transition-colors",
							active ? "text-white" : "text-zinc-500 hover:text-zinc-300",
						)}
					>
						{active && (
							<motion.span
								layoutId={layoutId}
								className="absolute inset-0 rounded-md bg-zinc-800"
								transition={{ type: "spring", bounce: 0.15, duration: 0.3 }}
							/>
						)}
						<span className="relative">{series.short}</span>
					</button>
				);
			})}
		</div>
	);
}
