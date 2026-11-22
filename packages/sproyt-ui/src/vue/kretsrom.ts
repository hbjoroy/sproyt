import { defineComponent, h, type PropType } from "vue";

export const KretsromShell = defineComponent({
  name: "KretsromShell",
  props: { view: { type: String as PropType<"list" | "detail">, default: "detail" } },
  setup(props, { slots }) {
    return () => h("div", { class: "kr-shell", "data-view": props.view }, [
      h("header", { class: "kr-mast" }, slots.masthead?.()),
      h("div", { class: "kr-layout" }, [
        h("nav", { class: "kr-directory", "aria-label": "Samtalar" }, slots.directory?.()),
        h("main", { class: "kr-room" }, [slots.roomHeader?.(), slots.default?.()])
      ])
    ]);
  }
});

export const KretsromMessage = defineComponent({
  name: "KretsromMessage",
  props: { author: String, time: String, dateTime: String, status: String },
  setup(props, { slots }) {
    return () => h("article", { class: "kr-message" }, [
      h("span", { class: "kr-avatar", "aria-hidden": "true" }, slots.avatar?.()),
      h("div", { class: "kr-copy" }, [
        h("header", { class: "kr-byline" }, [slots.author?.() ?? h("strong", props.author),
          h("time", { class: "sp-time", datetime: props.dateTime }, props.time), props.status && h("small", props.status)]),
        h("div", { class: "kr-body" }, slots.default?.()),
        h("div", { class: "kr-reactions" }, slots.reactions?.()),
        h("div", { class: "kr-actions" }, slots.actions?.())
      ])
    ]);
  }
});

export const KretsromComposerRow = defineComponent({
  name: "KretsromComposerRow",
  setup(_, { slots }) {
    return () => h("div", { class: "kr-compose" }, [slots.tools?.(), slots.default?.(), slots.send?.()]);
  }
});
