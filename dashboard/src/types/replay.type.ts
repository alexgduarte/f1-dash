export type ArchiveSession = {
	key: number;
	name: string;
	kind: string;
	path: string;
	start: string;
	gmtOffset: string;
};

export type ArchiveMeeting = {
	key: number;
	name: string;
	location: string;
	country: string;
	sessions: ArchiveSession[];
};

/** Published by replays as the `Replay` topic. */
export type ReplayStatus = {
	Path: string;
	Position: number;
	Start: number;
	End: number;
	Speed: number;
	Paused: boolean;
	Ended: boolean;
};
