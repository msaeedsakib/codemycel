import { outputDir } from "./build";

export default function Banner({ title }) {
  return (
    <header className="banner">
      {title} {outputDir(".")}
    </header>
  );
}

export const Footer = () => (
  <footer>
    <Banner title="end" />
  </footer>
);
