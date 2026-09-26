import { utc } from "moment";

import Countdown from "@/components/schedule/Countdown";
import Round from "@/components/schedule/Round";

import type { Round as RoundType, Session } from "@/types/schedule.type";

type Props = {
	next: RoundType | null;
};

// the headline race of a weekend: "Race" in F1 and WEC, "Feature Race" in F2 and F3
const findMainRace = (sessions: Session[]): Session | undefined =>
	sessions.findLast((s) => s.kind.toLowerCase().includes("race") && !s.kind.toLowerCase().includes("sprint")) ??
	sessions.findLast((s) => s.kind.toLowerCase().includes("race"));

export default function NextRound({ next }: Props) {
	if (!next) {
		return (
			<div className="flex h-44 flex-col items-center justify-center">
				<p>No upcoming weekend found</p>
			</div>
		);
	}

	const nextRace = findMainRace(next.sessions);
	const nextSession = next.sessions.filter((s) => utc(s.start) > utc() && s !== nextRace)[0];

	return (
		<div className="grid grid-cols-1 gap-8 sm:grid-cols-2">
			{nextSession || nextRace ? (
				<div className="flex flex-col gap-4">
					{nextSession && <Countdown next={nextSession} type="other" />}
					{nextRace && <Countdown next={nextRace} type="race" />}
				</div>
			) : (
				<div className="flex flex-col items-center justify-center">
					<p>No upcoming sessions found</p>
				</div>
			)}

			<Round round={next} />
		</div>
	);
}
