import Round from "@/components/schedule/Round";

import type { Round as RoundType } from "@/types/schedule.type";

type Props = {
	schedule: RoundType[] | null;
	next: RoundType | null;
};

export default function Schedule({ schedule, next }: Props) {
	if (!schedule || schedule.length === 0) {
		return (
			<div className="flex h-44 flex-col items-center justify-center">
				<p>Schedule not found</p>
			</div>
		);
	}

	return (
		<div className="mb-20 grid grid-cols-1 gap-8 md:grid-cols-2">
			{schedule.map((round, roundI) => (
				<Round nextName={next?.name} round={round} key={`round.${roundI}`} />
			))}
		</div>
	);
}
