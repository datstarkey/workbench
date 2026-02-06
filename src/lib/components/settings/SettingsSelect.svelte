<script lang="ts">
	import * as Select from '$lib/components/ui/select';

	let {
		label,
		description,
		options,
		value,
		onValueChange,
		triggerClass = 'w-40'
	}: {
		label: string;
		description: string;
		options: Array<{ value: string; label: string }>;
		value: string;
		onValueChange: (value: string) => void;
		triggerClass?: string;
	} = $props();
</script>

<div>
	<h3 class="text-sm font-medium">{label}</h3>
	<p class="mt-1 text-xs text-muted-foreground">{description}</p>
	<div class="mt-2">
		<Select.Root type="single" {value} {onValueChange}>
			<Select.Trigger class={triggerClass}>
				{options.find((o) => o.value === value)?.label ?? value}
			</Select.Trigger>
			<Select.Content>
				{#each options as opt (opt.value)}
					<Select.Item value={opt.value}>{opt.label}</Select.Item>
				{/each}
			</Select.Content>
		</Select.Root>
	</div>
</div>
