import type { Meta, StoryObj } from "@storybook/svelte-vite";
import WizardWelcome from "./WizardWelcome.svelte";

const meta = {
  title: "Wizard/Welcome",
  component: WizardWelcome,
  tags: ["autodocs"],
  parameters: { layout: "centered" },
} satisfies Meta<typeof WizardWelcome>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
