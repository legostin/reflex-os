import type { ButtonHTMLAttributes, HTMLAttributes, InputHTMLAttributes, ReactNode } from "react";

export function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}

const focusRing =
  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-reflex-accent/70 focus-visible:ring-offset-0";

export function Button({
  className,
  variant = "secondary",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: "primary" | "secondary" | "ghost" | "danger" }) {
  const variants = {
    primary: "border-reflex-accent/50 bg-reflex-accent/20 text-white hover:bg-reflex-accent/28",
    secondary: "border-white/10 bg-white/[0.045] text-white/80 hover:bg-white/[0.075]",
    ghost: "border-transparent bg-transparent text-white/62 hover:bg-white/[0.06] hover:text-white/86",
    danger: "border-red-400/35 bg-red-500/12 text-red-100 hover:bg-red-500/18",
  };

  return (
    <button
      {...props}
      className={cx(
        "inline-flex min-h-8 items-center justify-center gap-2 rounded-md border px-3 py-1.5 text-xs font-medium transition disabled:cursor-not-allowed disabled:opacity-45",
        focusRing,
        variants[variant],
        className,
      )}
    />
  );
}

export function IconButton({ className, ...props }: ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      {...props}
      className={cx(
        "inline-flex size-8 items-center justify-center rounded-md border border-white/10 bg-white/[0.04] text-white/70 transition hover:bg-white/[0.075] hover:text-white disabled:cursor-not-allowed disabled:opacity-45",
        focusRing,
        className,
      )}
    />
  );
}

export function Badge({ className, ...props }: HTMLAttributes<HTMLSpanElement>) {
  return (
    <span
      {...props}
      className={cx(
        "inline-flex items-center rounded-md border border-white/10 bg-white/[0.045] px-2 py-0.5 text-[11px] font-medium text-white/62",
        className,
      )}
    />
  );
}

export function Panel({ className, ...props }: HTMLAttributes<HTMLElement>) {
  return <section {...props} className={cx("rounded-md border border-white/10 bg-white/[0.035]", className)} />;
}

export function TextInput({ className, ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      className={cx(
        "min-h-8 rounded-md border border-white/10 bg-black/20 px-3 py-1.5 text-sm text-white placeholder:text-white/32",
        focusRing,
        className,
      )}
    />
  );
}

export function ModalFrame({
  title,
  children,
  footer,
  className,
}: {
  title: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  className?: string;
}) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-6">
      <section
        className={cx(
          "max-h-[86vh] w-full max-w-2xl overflow-hidden rounded-md border border-white/12 bg-reflex-panel shadow-2xl",
          className,
        )}
      >
        <header className="border-b border-white/10 px-5 py-4 text-base font-semibold text-white">{title}</header>
        <div className="max-h-[65vh] overflow-auto p-5">{children}</div>
        {footer ? <footer className="border-t border-white/10 px-5 py-4">{footer}</footer> : null}
      </section>
    </div>
  );
}
