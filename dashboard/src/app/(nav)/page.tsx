import Image from "next/image";
import Link from "next/link";

import Button from "@/components/ui/Button";
import ScrollHint from "@/components/ScrollHint";

import icon from "public/tag-logo.svg";

const RELEASES = "https://github.com/alexgduarte/f1-dash/releases/latest";

const features = [
	{
		title: "Five championships",
		body: "Formula 1, FIA Formula 2, FIA Formula 3, F1 Academy and the FIA World Endurance Championship, switchable from the sidebar. WEC timing shows class positions and the crew sharing each car.",
	},
	{
		title: "Tyre sets for the race",
		body: "Every Formula 1 driver's remaining sets, new or used with their lap count, rebuilt from all sessions of the weekend. Sets handed back to Pirelli after each session are taken out following the FIA return rules.",
	},
	{
		title: "Weather radar",
		body: "Live rain radar around the circuit with a timeline, next to track and air temperature, humidity, rain and a wind arrow.",
	},
	{
		title: "Session replay",
		body: "Watch any finished Formula 1 session since 2018 again, with play, pause, up to 32× speed and a timeline to jump around.",
	},
	{
		title: "Timing, map and race control",
		body: "Leaderboard with gaps, sectors and mini sectors, a track map with approximate car positions, race control messages, team radio and track limit tracking.",
	},
];

const platforms = ["Windows", "macOS (Apple Silicon and Intel)", "Linux", "iPhone and iPad", "Android", "Web"];

export default function Home() {
	return (
		<div>
			<section className="flex h-screen w-full flex-col items-center pt-20 sm:justify-center sm:pt-0">
				<Image src={icon} alt="f1-dash tag logo" width={200} />

				<h1 className="my-20 text-center text-5xl font-bold">
					Real-time motorsport <br />
					telemetry and timing
				</h1>

				<div className="flex flex-wrap justify-center gap-4">
					<Link href="/dashboard">
						<Button className="rounded-xl! border-2 border-transparent p-4 font-medium">Go to Dashboard</Button>
					</Link>

					<a href={RELEASES} target="_blank">
						<Button className="rounded-xl! border-2 border-zinc-700 bg-transparent! p-4 font-medium">
							Download the app
						</Button>
					</a>

					<Link href="/schedule">
						<Button className="rounded-xl! border-2 border-zinc-700 bg-transparent! p-4 font-medium">
							Check Schedule
						</Button>
					</Link>
				</div>

				<ScrollHint />
			</section>

			<section className="pb-20">
				<h2 className="mb-4 text-2xl">What&apos;s f1-dash?</h2>

				<p className="text-md">
					f1-dash is a real-time timing dashboard for motorsport. It shows live timing, tyres, gaps, lap and sector
					times, race control and the weather, in the browser or as an app.
				</p>
			</section>

			<section className="grid grid-cols-1 gap-6 pb-20 sm:grid-cols-2">
				{features.map((feature) => (
					<div key={feature.title} className="rounded-xl border border-zinc-800 p-4">
						<h3 className="mb-2 text-lg font-semibold">{feature.title}</h3>
						<p className="text-sm text-zinc-400">{feature.body}</p>
					</div>
				))}
			</section>

			<section className="pb-20">
				<h2 className="mb-4 text-2xl">Apps</h2>

				<p className="text-md mb-4">
					The apps connect to the timing feeds straight from your device, so they work without any f1-dash server.
				</p>

				<ul className="flex flex-wrap gap-2">
					{platforms.map((platform) => (
						<li key={platform} className="rounded-full border border-zinc-800 px-3 py-1 text-sm text-zinc-300">
							{platform}
						</li>
					))}
				</ul>
			</section>

			<section className="pb-20">
				<h2 className="mb-4 text-2xl">What happened to the position data and car metrics?</h2>

				<p className="text-md">
					Formula 1 put the car position and telemetry feeds behind an F1 TV subscription in 2025. f1-dash does not log
					in to F1 accounts, so car positions on the track map are estimated from mini sector timing instead. That is
					less precise than GPS positions but still shows where everyone is.
				</p>
			</section>

			<section className="pb-20">
				<h2 className="mb-4 text-2xl">Where does it come from?</h2>

				<p className="text-md">
					This is a fork of{" "}
					<a className="text-blue-500" target="_blank" href="https://github.com/slowlydev/f1-dash">
						f1-dash
					</a>{" "}
					by slowlydev, who has since moved on to building{" "}
					<a className="text-blue-500" target="_blank" href="https://nitrous.software">
						Nitrous
					</a>
					. It adds more championships, native apps, tyre set tracking and a weather radar.
				</p>
			</section>
		</div>
	);
}
