import { ToggleGroup as ToggleGroupPrimitive } from 'radix-ui'
import type * as React from 'react'
import { cn } from '@/lib/utils'

/** Segmented control. */
function ToggleGroup({ className, ...props }: React.ComponentProps<typeof ToggleGroupPrimitive.Root>) {
  return <ToggleGroupPrimitive.Root className={cn('inline-flex h-8 items-center gap-0.5 rounded-[9px] border border-line bg-bg3/70 p-[3px]', className)} {...props} />
}
function ToggleGroupItem({ className, ...props }: React.ComponentProps<typeof ToggleGroupPrimitive.Item>) {
  return (
    <ToggleGroupPrimitive.Item
      className={cn(
        'pressable inline-flex h-full flex-1 items-center justify-center gap-1.5 rounded-[6px] px-2.5 text-xs font-medium whitespace-nowrap text-ink2 outline-none data-[state=on]:bg-bg2 data-[state=on]:text-ink data-[state=on]:shadow-[0_1px_2px_rgba(0,0,0,0.18),inset_0_0_0_1px_var(--line2)] [&_svg]:size-3.5',
        className,
      )}
      {...props}
    />
  )
}
export { ToggleGroup, ToggleGroupItem }
