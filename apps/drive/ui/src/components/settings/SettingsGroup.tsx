import { cn } from "@/lib/utils";

export function SettingsGroup({
  title,
  description,
  children,
  className,
}: {
  title: string;
  description?: string;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <section className={cn("settings-group", className)}>
      <header className="settings-group-header">
        <h2>{title}</h2>
        {description ? <p>{description}</p> : null}
      </header>
      <div className="settings-group-body">{children}</div>
    </section>
  );
}
