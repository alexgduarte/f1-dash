import clsx from "clsx";

import type { Compound } from "@/types/state.type";

export const compoundColor: Record<Compound, string> = {
	SOFT: "#F12F32",
	MEDIUM: "#FBCC1C",
	HARD: "#FFFFFF",
	INTERMEDIATE: "#128330",
	WET: "#1F6DA1",
};

const compoundLetter: Record<Compound, string> = {
	SOFT: "S",
	MEDIUM: "M",
	HARD: "H",
	INTERMEDIATE: "I",
	WET: "W",
};

type Props = {
	compound: Compound;
	/** shown under the tyre: "NEW" or the lap count */
	label?: string;
	size?: number;
	dimmed?: boolean;
	highlighted?: boolean;
	title?: string;
};

export default function TyreBadge({ compound, label, size = 24, dimmed, highlighted, title }: Props) {
	const color = compoundColor[compound];

	return (
		<div className={clsx("flex flex-col items-center gap-0.5", dimmed && "opacity-30")} title={title}>
			<svg
				width={size}
				height={size}
				viewBox="0 0 24 24"
				role="img"
				aria-label={title ?? compound}
				className={clsx("rounded-full", highlighted && "ring-2 ring-sky-400 ring-offset-1 ring-offset-zinc-950")}
			>
				<circle cx="12" cy="12" r="12" fill="black" />
				<path d="M9.8 2.7A9.6 9.6 0 0 0 9.8 21.3" stroke={color} strokeWidth="2.6" fill="none" />
				<path d="M14.2 21.3A9.6 9.6 0 0 0 14.2 2.7" stroke={color} strokeWidth="2.6" fill="none" />
				<text x="12" y="12.5" textAnchor="middle" dominantBaseline="middle" fontSize="10" fontWeight="800" fill="white">
					{compoundLetter[compound]}
				</text>
			</svg>

			{label !== undefined && (
				<p className="text-[10px] leading-none font-semibold text-zinc-400 tabular-nums">{label}</p>
			)}
		</div>
	);
}
