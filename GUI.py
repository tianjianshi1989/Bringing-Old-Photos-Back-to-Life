import os
import sys
import shutil
import subprocess
import threading
import tkinter as tk
from tkinter import filedialog, messagebox
from PIL import Image, ImageOps, ImageTk

PROJECT_ROOT = os.path.dirname(os.path.abspath(__file__))
GUI_BUILD = "2026-02-02"

def _prepare_single_image_folder(input_image_path, output_folder):
    input_dir = os.path.join(output_folder, "_gui_input")
    if os.path.exists(input_dir):
        shutil.rmtree(input_dir)
    os.makedirs(input_dir, exist_ok=True)
    shutil.copy(input_image_path, os.path.join(input_dir, os.path.basename(input_image_path)))
    return input_dir


def _pick_latest_file(dir_path):
    if not os.path.isdir(dir_path):
        return None
    candidates = [
        os.path.join(dir_path, f)
        for f in os.listdir(dir_path)
        if f and not f.startswith(".") and os.path.isfile(os.path.join(dir_path, f))
    ]
    if not candidates:
        return None
    return max(candidates, key=os.path.getmtime)


def modify(input_path, output_folder=None, gpu="-1", with_scratch=True, hr=False):
    output_folder = output_folder or os.path.join(PROJECT_ROOT, "output_gui")
    os.makedirs(output_folder, exist_ok=True)

    if os.path.isdir(input_path):
        input_folder = os.path.abspath(input_path)
    else:
        input_folder = _prepare_single_image_folder(os.path.abspath(input_path), output_folder)

    cmd = [
        sys.executable,
        os.path.join(PROJECT_ROOT, "run.py"),
        "--input_folder",
        input_folder,
        "--output_folder",
        os.path.abspath(output_folder),
        "--GPU",
        str(gpu),
    ]
    if with_scratch:
        cmd.append("--with_scratch")
    if hr:
        cmd.append("--HR")

    subprocess.check_call(cmd, cwd=PROJECT_ROOT)
    return _pick_latest_file(os.path.join(output_folder, "final_output"))

class App:
    def __init__(self, root):
        self.root = root
        self.root.title(f"Bringing-old-photos-back-to-life (GUI {GUI_BUILD})")
        self.root.minsize(1100, 650)

        self.input_path_var = tk.StringVar(value="")
        self.with_scratch_var = tk.BooleanVar(value=True)
        self.hr_var = tk.BooleanVar(value=False)
        self.status_var = tk.StringVar(value="Ready")

        self._in_photo = None
        self._out_photo = None

        self.root.grid_rowconfigure(3, weight=1)
        self.root.grid_columnconfigure(0, weight=1)
        self.root.grid_columnconfigure(1, weight=1)

        top = tk.Frame(root)
        top.grid(row=0, column=0, columnspan=2, sticky="ew", padx=10, pady=(10, 6))
        top.grid_columnconfigure(1, weight=1)

        tk.Label(top, text="Input file:", fg="black").grid(row=0, column=0, sticky="w")
        self.path_entry = tk.Entry(top, textvariable=self.input_path_var, highlightthickness=1, highlightbackground="#999999")
        self.path_entry.grid(row=0, column=1, sticky="ew", padx=(6, 6))
        tk.Button(top, text="Browse", command=self.on_browse).grid(row=0, column=2, sticky="e")

        opts = tk.Frame(root)
        opts.grid(row=1, column=0, columnspan=2, sticky="ew", padx=10, pady=(0, 6))
        tk.Checkbutton(opts, text="With scratch", variable=self.with_scratch_var).grid(row=0, column=0, sticky="w")
        tk.Checkbutton(opts, text="HR", variable=self.hr_var).grid(row=0, column=1, sticky="w", padx=(10, 0))

        actions = tk.Frame(root)
        actions.grid(row=2, column=0, columnspan=2, sticky="ew", padx=10, pady=(0, 10))
        actions.grid_columnconfigure(2, weight=1)

        self.run_btn = tk.Button(actions, text="Modify Photo", command=self.on_run)
        self.run_btn.grid(row=0, column=0, sticky="w")
        tk.Button(actions, text="Exit", command=self.root.destroy).grid(row=0, column=1, sticky="w", padx=(10, 0))
        tk.Label(actions, textvariable=self.status_var, fg="black").grid(row=0, column=2, sticky="w", padx=(10, 0))

        imgs = tk.Frame(root)
        imgs.grid(row=3, column=0, columnspan=2, sticky="nsew", padx=10, pady=(0, 10))
        imgs.grid_rowconfigure(0, weight=1)
        imgs.grid_columnconfigure(0, weight=1)
        imgs.grid_columnconfigure(1, weight=1)

        self.in_canvas = tk.Canvas(imgs, width=520, height=520, bg="white", highlightthickness=2, highlightbackground="#666666")
        self.in_canvas.grid(row=0, column=0, sticky="nsew", padx=(0, 10))

        self.out_canvas = tk.Canvas(imgs, width=520, height=520, bg="white", highlightthickness=2, highlightbackground="#666666")
        self.out_canvas.grid(row=0, column=1, sticky="nsew")

        self._init_canvas(self.in_canvas, "Input Preview")
        self._init_canvas(self.out_canvas, "Output Preview")

        self.status_bar = tk.Label(root, textvariable=self.status_var, anchor="w", relief=tk.SUNKEN, bg="#ffffe0")
        self.status_bar.grid(row=4, column=0, columnspan=2, sticky="ew")

    def on_browse(self):
        path = filedialog.askopenfilename(
            parent=self.root,
            filetypes=[
                ("Images", "*.jpg *.jpeg *.png *.bmp *.tif *.tiff"),
                ("All files", "*.*"),
            ],
        )
        if not path:
            return
        path = str(path)
        self.root.after(0, lambda p=path: self._on_file_selected(p))

    def _on_file_selected(self, path):
        self.input_path_var.set(path)
        self.status_var.set(f"Selected: {os.path.basename(path)}")
        self.root.title(f"Bringing-old-photos-back-to-life (GUI {GUI_BUILD}) - {os.path.basename(path)}")
        try:
            self._set_canvas_image(self.in_canvas, path, is_output=False)
        except Exception as e:
            messagebox.showerror("Error", str(e))

    def on_run(self):
        input_path = self.input_path_var.get().strip()
        if not input_path:
            messagebox.showerror("Error", "Please choose an input file first.")
            return
        if not os.path.exists(input_path):
            messagebox.showerror("Error", f"Input not found: {input_path}")
            return

        self.run_btn.config(state=tk.DISABLED)
        self.status_var.set("Running...")

        def task():
            try:
                output_path = modify(
                    input_path,
                    output_folder=os.path.join(PROJECT_ROOT, "output_gui"),
                    gpu="-1",
                    with_scratch=bool(self.with_scratch_var.get()),
                    hr=bool(self.hr_var.get()),
                )
                if not output_path:
                    raise RuntimeError("No output image found under output_gui/final_output")
                self.root.after(0, lambda: self._on_done(output_path, None))
            except Exception as e:
                self.root.after(0, lambda: self._on_done(None, e))

        threading.Thread(target=task, daemon=True).start()

    def _on_done(self, output_path, error):
        self.run_btn.config(state=tk.NORMAL)
        self.status_var.set("")
        if error is not None:
            messagebox.showerror("Error", str(error))
            return
        try:
            self._set_canvas_image(self.out_canvas, output_path, is_output=True)
        except Exception as e:
            messagebox.showerror("Error", str(e))

    def _init_canvas(self, canvas, title):
        canvas.delete("all")
        canvas.create_rectangle(1, 1, 10000, 10000, outline="")
        canvas.create_text(10, 10, anchor="nw", text=title, fill="black", font=("Helvetica", 14, "bold"), tags=("title",))

    def _set_canvas_image(self, canvas, path, is_output):
        canvas.delete("img")
        self.root.update_idletasks()
        img = Image.open(path)
        img = ImageOps.exif_transpose(img)
        img = img.convert("RGB")
        fallback_w = int(canvas.cget("width") or 520)
        fallback_h = int(canvas.cget("height") or 520)
        canvas_width = max(int(canvas.winfo_width()), fallback_w, 520)
        canvas_height = max(int(canvas.winfo_height()), fallback_h, 520)
        if canvas_width < 200 or canvas_height < 200:
            canvas_width = max(canvas_width, 800)
            canvas_height = max(canvas_height, 600)
        max_w = max(canvas_width - 20, 100)
        max_h = max(canvas_height - 60, 100)
        img.thumbnail((max_w, max_h))
        photo = ImageTk.PhotoImage(img, master=self.root)
        canvas.create_image(canvas_width // 2, canvas_height // 2 + 10, image=photo, anchor="center", tags=("img",))
        canvas.create_text(10, 40, anchor="nw", text=os.path.basename(path), fill="black", font=("Helvetica", 12), tags=("img",))
        canvas.image = photo
        canvas.update_idletasks()
        if is_output:
            self._out_photo = photo
        else:
            self._in_photo = photo


if __name__ == "__main__":
    root = tk.Tk()
    App(root)
    root.mainloop()
