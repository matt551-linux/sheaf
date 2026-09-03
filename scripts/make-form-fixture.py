# Generates tests/fixtures/form.pdf: an AcroForm with the field types M3 covers.
from reportlab.pdfgen import canvas
from reportlab.lib.pagesizes import letter

OUT = "tests/fixtures/form.pdf"
c = canvas.Canvas(OUT, pagesize=letter)
w, h = letter

c.setFont("Helvetica-Bold", 16)
c.drawString(72, h - 72, "Sheaf form fixture")
c.setFont("Helvetica", 11)

form = c.acroForm

c.drawString(72, h - 118, "Name (required):")
form.textfield(name="name", tooltip="Full name", x=200, y=h - 132, width=250, height=20,
               borderWidth=1, forceBorder=True, fieldFlags="required")

c.drawString(72, h - 158, "Email:")
form.textfield(name="email", tooltip="Email", x=200, y=h - 172, width=250, height=20,
               borderWidth=1, forceBorder=True)

c.drawString(72, h - 198, "Comments (multiline):")
form.textfield(name="comments", x=200, y=h - 252, width=250, height=60,
               borderWidth=1, forceBorder=True, fieldFlags="multiline")

c.drawString(72, h - 288, "Subscribe:")
form.checkbox(name="subscribe", x=200, y=h - 292, size=16, borderWidth=1, forceBorder=True)

c.drawString(72, h - 322, "Color:")
form.radio(name="color", value="red", x=200, y=h - 326, size=16, borderWidth=1, forceBorder=True)
c.drawString(222, h - 322, "red")
form.radio(name="color", value="green", x=260, y=h - 326, size=16, borderWidth=1, forceBorder=True)
c.drawString(282, h - 322, "green")
form.radio(name="color", value="blue", x=330, y=h - 326, size=16, borderWidth=1, forceBorder=True)
c.drawString(352, h - 322, "blue")

c.drawString(72, h - 356, "Size (combo):")
form.choice(name="size", value="medium", x=200, y=h - 364, width=120, height=20,
            options=[("Small", "small"), ("Medium", "medium"), ("Large", "large")],
            borderWidth=1, forceBorder=True, fieldFlags="combo")

c.drawString(72, h - 392, "Toppings (listbox):")
form.listbox(name="toppings", value="cheese", x=200, y=h - 440, width=120, height=52,
             options=[("Cheese", "cheese"), ("Olives", "olives"), ("Onion", "onion")],
             borderWidth=1, forceBorder=True, fieldFlags="multiSelect")

c.save()
print("wrote", OUT)
