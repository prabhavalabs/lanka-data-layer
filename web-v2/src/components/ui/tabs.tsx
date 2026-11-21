import { Tabs as TabsPrimitive } from 'radix-ui'
import type * as React from 'react'
import { cn } from '@/lib/utils'

function Tabs({ className, ...props }: React.ComponentProps<typeof TabsPrimitive.Root>) {
  return <TabsPrimitive.Root data-slot="tabs" className={cn('flex flex-col gap-3', className)} {...props} />
}
function TabsList({ className, ...props }: React.ComponentProps<typeof TabsPrimitive.List>) {
  return (
    <TabsPrimitive.List
      data-slot="tabs-list"
      className={cn('inline-flex h-9 w-fit items-center gap-0.5 rounded-[10px] border border-line bg-bg3/70 p-[3px]', className)}
      {...props}
    />
  )
}
function TabsTrigger({ className, ...props }: React.ComponentProps<typeof TabsPrimitive.Trigger>) {
  return (
    <TabsPrimitive.Trigger
      data-slot="tabs-trigger"
      className={cn(
        'pressable inline-flex h-full items-center justify-center gap-1.5 rounded-[7px] px-3 text-[13px] font-medium whitespace-nowrap text-ink2 outline-none data-[state=active]:bg-bg2 data-[state=active]:text-ink data-[state=active]:shadow-[0_1px_2px_rgba(0,0,0,0.18),inset_0_0_0_1px_var(--line2)] [&_svg]:size-3.5 [@media(hover:hover)_and_(pointer:fine)]:hover:text-ink',
        className,
      )}
      {...props}
    />
  )
}
function TabsContent({ className, ...props }: React.ComponentProps<typeof TabsPrimitive.Content>) {
  return <TabsPrimitive.Content data-slot="tabs-content" className={cn('outline-none', className)} {...props} />
}
export { Tabs, TabsList, TabsTrigger, TabsContent }
