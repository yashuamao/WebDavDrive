import { Switch as SwitchPrimitive } from "@base-ui/react/switch";
import { cn } from "@/lib/utils";

interface SwitchProps extends React.ComponentPropsWithoutRef<typeof SwitchPrimitive.Root> {
  label: string;
}

export function Switch({ className, label, ...props }: SwitchProps) {
  return (
    <SwitchPrimitive.Root
      className={cn(
        "relative inline-flex h-5 w-9 shrink-0 items-center rounded-full border border-border-strong bg-control-strong transition-colors duration-150 data-[checked]:border-accent data-[checked]:bg-accent focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus",
        className,
      )}
      aria-label={label}
      {...props}
    >
      {/* 圆点 14px、轨道内框 18px：靠 items-center 垂直居中，否则默认顶对齐会
          让它贴着上沿（上 0px / 下 4px），看起来就是「圆圈位置不居中」。
          选中态 18px = 左右各留 2px，与未选中态对称。 */}
      <SwitchPrimitive.Thumb className="block size-3.5 translate-x-0.5 rounded-full bg-white shadow-sm transition-transform duration-150 data-[checked]:translate-x-[18px]" />
    </SwitchPrimitive.Root>
  );
}
