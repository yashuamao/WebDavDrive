import { forwardRef, type ButtonHTMLAttributes } from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

export const buttonVariants = cva(
  "inline-flex min-w-0 items-center justify-center gap-2 whitespace-nowrap rounded-md border text-sm font-medium transition-[background-color,border-color,color,box-shadow] duration-150 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus disabled:pointer-events-none disabled:opacity-45",
  {
    variants: {
      variant: {
        primary: "border-accent bg-accent text-accent-foreground hover:bg-accent-hover active:bg-accent-active",
        default: "border-border-strong bg-control text-text-primary hover:bg-control-hover active:bg-control-active",
        subtle: "border-transparent bg-transparent text-text-secondary hover:bg-control-hover hover:text-text-primary active:bg-control-active",
        danger: "border-danger bg-danger text-white hover:bg-danger-hover",
        ghostDanger: "border-transparent bg-transparent text-danger hover:bg-danger-muted",
      },
      size: {
        default: "h-8 px-3",
        compact: "h-7 px-2.5 text-[13px]",
        icon: "size-8 p-0",
      },
    },
    defaultVariants: { variant: "default", size: "default" },
  },
);

export interface ButtonProps
  extends ButtonHTMLAttributes<HTMLButtonElement>, VariantProps<typeof buttonVariants> {}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, type = "button", ...props }, ref) => (
    <button ref={ref} type={type} className={cn(buttonVariants({ variant, size }), className)} {...props} />
  ),
);
Button.displayName = "Button";
